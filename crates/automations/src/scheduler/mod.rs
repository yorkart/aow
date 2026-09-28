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

mod dispatch;
mod render;
mod sync;

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
            "请先通过 GitHub Releases 安装 AoW"
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
}

#[cfg(test)]
mod tests;
