use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tokio::process::Command;

use crate::{
    ManualRunRequest, RunSource, Schedule, Store, Task, TaskKind,
    store::{atomic_write, new_run_id, valid_component},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Systemd,
    Launchd,
    Unsupported,
}

#[derive(Clone)]
pub struct Scheduler {
    pub platform: Platform,
    pub runner: PathBuf,
    pub directory: PathBuf,
    pub manager_command: PathBuf,
    pub dispatch_command: PathBuf,
}

#[derive(Serialize)]
pub struct SchedulerStatus {
    pub platform: Platform,
    pub ready: bool,
    pub message: Option<String>,
    pub timezone: String,
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn unit_arg(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

impl Scheduler {
    pub fn new(home: &Path) -> Self {
        let runner = std::env::var_os("AOW_AUTOMATION_RUNNER")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/bin/aow-automation-runner"));
        let platform = if cfg!(target_os = "linux") {
            Platform::Systemd
        } else if cfg!(target_os = "macos") {
            Platform::Launchd
        } else {
            Platform::Unsupported
        };
        let directory = if platform == Platform::Launchd {
            home.join("Library/LaunchAgents")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("systemd/user")
        };
        Self {
            platform,
            runner,
            directory,
            manager_command: if platform == Platform::Launchd {
                "launchctl".into()
            } else {
                "systemctl".into()
            },
            dispatch_command: if platform == Platform::Launchd {
                "launchctl".into()
            } else {
                "systemd-run".into()
            },
        }
    }

    async fn command(&self, executable: &Path, args: &[String]) -> Result<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            Command::new(executable)
                .args(args)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("系统定时器操作超时")??;
        ensure!(
            output.status.success(),
            "系统定时器操作失败: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        );
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub async fn status(&self) -> SchedulerStatus {
        let result = self.check_ready().await;
        SchedulerStatus {
            platform: self.platform,
            ready: result.is_ok(),
            message: result.err().map(|e| e.to_string()),
            timezone: chrono::Local::now().format("%Z (UTC%:z)").to_string(),
        }
    }

    async fn check_ready(&self) -> Result<()> {
        ensure!(
            self.platform != Platform::Unsupported,
            "自动化支持 Linux systemd 和 macOS launchd"
        );
        ensure!(
            self.runner.is_absolute()
                && fs::metadata(&self.runner)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0),
            "请先通过 GitHub Releases 安装 AOW"
        );
        let args = if self.platform == Platform::Systemd {
            vec!["--user".into(), "show-environment".into()]
        } else {
            vec![
                "print".into(),
                format!("gui/{}", unsafe { libc::geteuid() }),
            ]
        };
        self.command(&self.manager_command, &args).await?;
        Ok(())
    }

    fn label(&self, id: &str) -> String {
        if self.platform == Platform::Launchd {
            format!("org.aow.automation.{id}")
        } else {
            format!("aow-automation-{id}")
        }
    }

    fn arguments(
        &self,
        store: &Store,
        task: &Task,
        action: &str,
        source: RunSource,
    ) -> Vec<String> {
        vec![
            self.runner.to_string_lossy().into_owned(),
            action.into(),
            "--state-dir".into(),
            store.state_dir.to_string_lossy().into_owned(),
            "--task-id".into(),
            task.id.clone(),
            "--source".into(),
            if source == RunSource::Manual {
                "manual"
            } else {
                "scheduled"
            }
            .into(),
        ]
    }

    pub fn render(&self, store: &Store, task: &Task) -> Result<Vec<(PathBuf, String)>> {
        valid_component(&task.id)?;
        task.input.validate_schedule()?;
        if task.input.kind == TaskKind::Manual {
            return Ok(Vec::new());
        }
        let label = self.label(&task.id);
        let args = self.arguments(store, task, "trigger", RunSource::Scheduled);
        match self.platform {
            Platform::Systemd => {
                let service = format!(
                    "[Unit]\nDescription=AOW automation trigger {}\n[Service]\nType=oneshot\nExecStart=:{}\nUMask=0077\nStandardOutput=null\nStandardError=journal\n",
                    task.id,
                    args.iter()
                        .map(|arg| unit_arg(arg))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let timer = format!(
                    "[Unit]\nDescription=AOW automation {}\n[Timer]\n{}\nPersistent=false\nAccuracySec=1s\n[Install]\nWantedBy=timers.target\n",
                    task.id,
                    if let Some(seconds) = task.input.interval_seconds {
                        format!("OnActiveSec={seconds}s\nOnUnitInactiveSec={seconds}s")
                    } else {
                        Schedule::parse(&task.input.cron)?
                            .systemd_calendars()
                            .iter()
                            .map(|v| format!("OnCalendar={v}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                );
                Ok(vec![
                    (self.directory.join(format!("{label}.service")), service),
                    (self.directory.join(format!("{label}.timer")), timer),
                ])
            }
            Platform::Launchd => {
                let plist = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array>{}</array>{}<key>RunAtLoad</key><false/><key>KeepAlive</key><false/><key>StandardOutPath</key><string>/dev/null</string><key>StandardErrorPath</key><string>/dev/null</string></dict></plist>\n",
                    args.iter()
                        .map(|arg| format!("<string>{}</string>", xml(arg)))
                        .collect::<String>(),
                    if let Some(seconds) = task.input.interval_seconds {
                        format!(
                            "<key>StartInterval</key><integer>{seconds}</integer><key>ThrottleInterval</key><integer>1</integer>"
                        )
                    } else {
                        format!(
                            "<key>StartCalendarInterval</key>{}",
                            Schedule::parse(&task.input.cron)?.launchd_calendar_xml()?
                        )
                    }
                );
                Ok(vec![(self.directory.join(format!("{label}.plist")), plist)])
            }
            Platform::Unsupported => anyhow::bail!("不支持的系统定时器"),
        }
    }

    pub async fn sync(&self, store: &Store, task: &Task) -> Result<()> {
        if task.input.kind == TaskKind::Manual {
            valid_component(&task.id)?;
            task.input.validate_schedule()?;
            // New manual tasks never contact the timer manager. Converting an
            // existing task removes its former trigger, without touching runs.
            let label = self.label(&task.id);
            let extensions = if self.platform == Platform::Systemd {
                vec!["timer", "service"]
            } else {
                vec!["plist"]
            };
            let paths: Vec<_> = extensions
                .iter()
                .map(|extension| self.directory.join(format!("{label}.{extension}")))
                .collect();
            if paths.iter().all(|path| !path.exists()) {
                return Ok(());
            }
            if self.platform == Platform::Systemd {
                if paths[0].exists() {
                    self.command(
                        &self.manager_command,
                        &[
                            "--user".into(),
                            "disable".into(),
                            "--now".into(),
                            format!("{label}.timer"),
                        ],
                    )
                    .await?;
                }
            } else {
                let target = format!("gui/{}/{label}", unsafe { libc::geteuid() });
                if self
                    .command(&self.manager_command, &["print".into(), target.clone()])
                    .await
                    .is_ok()
                {
                    self.command(&self.manager_command, &["bootout".into(), target])
                        .await?;
                }
            }
            for path in paths {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
            if self.platform == Platform::Systemd {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "daemon-reload".into()],
                )
                .await?;
            }
            return Ok(());
        }
        let files = self.render(store, task)?;
        let enabled = task.input.enabled && !task.deleted;
        if enabled {
            self.check_ready().await?;
        } else if files.iter().all(|(path, _)| !path.exists()) {
            return Ok(());
        }
        let label = self.label(&task.id);
        if self.platform == Platform::Systemd {
            // Only the short-lived trigger is replaced; actual runs have separate units.
            let timer = format!("{label}.timer");
            if self.directory.join(&timer).exists() {
                self.command(
                    &self.manager_command,
                    &[
                        "--user".into(),
                        "disable".into(),
                        "--now".into(),
                        timer.clone(),
                    ],
                )
                .await?;
            }
            for (path, content) in files {
                atomic_write(&path, content.as_bytes())?;
            }
            self.command(
                &self.manager_command,
                &["--user".into(), "daemon-reload".into()],
            )
            .await?;
            if enabled {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "enable".into(), "--now".into(), timer],
                )
                .await?;
            }
        } else {
            let domain = format!("gui/{}", unsafe { libc::geteuid() });
            let target = format!("{domain}/{label}");
            if self
                .command(&self.manager_command, &["print".into(), target.clone()])
                .await
                .is_ok()
            {
                self.command(&self.manager_command, &["bootout".into(), target])
                    .await?;
            }
            for (path, content) in files {
                if enabled {
                    atomic_write(&path, content.as_bytes())?;
                    self.command(
                        &self.manager_command,
                        &[
                            "bootstrap".into(),
                            domain.clone(),
                            path.to_string_lossy().into_owned(),
                        ],
                    )
                    .await?;
                } else if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
        if task.deleted {
            for (path, _) in self.render(store, task)? {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
            if self.platform == Platform::Systemd {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "daemon-reload".into()],
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn dispatch(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
    ) -> Result<Option<String>> {
        self.dispatch_with_variables(store, task, source, BTreeMap::new())
            .await
    }

    pub async fn dispatch_with_variables(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
        variables: BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        ensure!(
            source == RunSource::Manual || task.input.kind == TaskKind::Scheduled,
            "手动任务不能定时触发"
        );
        task.input.render_prompt(&variables)?;
        // This avoids creating a transient OS job when all slots are occupied.
        // The runner acquires a slot again as the authoritative race-free check.
        if store
            .concurrency_slot(&task.id, task.input.max_concurrent_runs)?
            .is_none()
        {
            return Ok(None);
        }
        let id = new_run_id();
        if source == RunSource::Manual {
            store.save_manual_request(
                &id,
                &ManualRunRequest {
                    task: task.clone(),
                    variables: variables.clone(),
                },
            )?;
        }
        if let Err(error) = self.dispatch_run(store, task, source, &id).await {
            if source == RunSource::Manual {
                let _ = store.take_manual_request(&task.id, &id);
            }
            // If the native manager failed before a runner claimed this ID,
            // leave a failed execution record. create_run never overwrites.
            let mut run = crate::runner::initial_run(task, id.clone(), source);
            if source == RunSource::Manual {
                run.variables = Some(variables);
            }
            if let Ok(mut writer) = store.create_run(&run, task) {
                writer.append(&crate::RunEvent::Finished {
                    at: chrono::Utc::now(),
                    status: crate::RunStatus::Failed,
                    exit_code: None,
                    message: Some(format!("Runner 启动失败: {error:#}")),
                    duration_ms: 0,
                })?;
            }
            return Err(error);
        }
        Ok(Some(id))
    }

    async fn dispatch_run(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
        id: &str,
    ) -> Result<()> {
        self.check_ready().await?;
        ensure!(
            !task.deleted && (source == RunSource::Manual || task.input.enabled),
            "任务已暂停或删除"
        );
        let mut args = self.arguments(store, task, "run", source);
        args.extend(["--run-id".into(), id.to_owned()]);
        let label = format!("{}.run.{id}", self.label(&task.id));
        if self.platform == Platform::Systemd {
            let mut launch = vec![
                "--user".into(),
                "--quiet".into(),
                "--collect".into(),
                "--service-type=exec".into(),
                format!("--unit={label}"),
                "--property=UMask=0077".into(),
                "--property=StandardOutput=null".into(),
                "--property=StandardError=journal".into(),
                "--".into(),
            ];
            launch.extend(args.into_iter().map(|arg| arg.replace('$', "$$")));
            self.command(&self.dispatch_command, &launch).await?;
        } else {
            // launchctl submit jobs stay registered after exit. Retire only our completed jobs.
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                for run in store.runs(&task.id, None, 100).unwrap_or_default() {
                    if run.status.terminal() {
                        let _ = self
                            .command(
                                &self.manager_command,
                                &[
                                    "remove".into(),
                                    format!("{}.run.{}", self.label(&task.id), run.id),
                                ],
                            )
                            .await;
                    }
                }
            })
            .await;
            let mut launch = vec![
                "submit".into(),
                "-l".into(),
                label,
                "-o".into(),
                "/dev/null".into(),
                "-e".into(),
                "/dev/null".into(),
                "--".into(),
            ];
            launch.extend(args);
            self.command(&self.dispatch_command, &launch).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_arguments_and_xml_are_literal() {
        assert_eq!(unit_arg("/a %h/\"$x"), "\"/a %%h/\\\"$x\"");
        assert_eq!(xml("<&\"'>"), "&lt;&amp;&quot;&apos;&gt;");
    }
}
