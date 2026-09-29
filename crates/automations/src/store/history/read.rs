use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde::Deserialize;

use crate::{Run, RunDetail, RunOutput, RunStatus, TaskInput};

use super::super::{FileLock, Store, is_not_found, try_shared, valid_component};
use super::order::compare_ids;

const MAX_RUN_EVENTS_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredRunEvent {
    Started {
        run: Box<Run>,
        configuration: TaskInput,
    },
    Workspace {
        path: PathBuf,
        #[serde(default)]
        branch: Option<String>,
        elapsed_ms: u64,
    },
    AgentStarted {
        pid: u32,
        #[serde(default)]
        command: Vec<String>,
    },
    Session {
        session_id: String,
        elapsed_ms: u64,
    },
    Finished {
        at: chrono::DateTime<Utc>,
        status: RunStatus,
        exit_code: Option<i32>,
        message: Option<String>,
        duration_ms: u64,
    },
    OutputTruncated {},
}
impl Store {
    pub fn read_run(&self, task_id: &str, run_id: &str) -> Result<Option<Run>> {
        Ok(self
            .read_run_detail(task_id, run_id)?
            .map(|detail| detail.run))
    }

    pub fn read_run_detail(&self, task_id: &str, run_id: &str) -> Result<Option<RunDetail>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.run_events_path(task_id, run_id)?)?;
        let live = !try_shared(&file)?;
        let mut file = FileLock {
            file,
            owns_lock: !live,
        };
        ensure!(
            file.metadata()?.len() <= MAX_RUN_EVENTS_BYTES,
            "执行记录超过大小限制"
        );
        ensure!(file.metadata()?.is_file(), "执行记录不是普通文件");
        let mut bytes = Vec::new();
        (&mut *file)
            .take(MAX_RUN_EVENTS_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_RUN_EVENTS_BYTES,
            "执行记录超过大小限制"
        );
        let mut run = None;
        let mut configuration = None;
        for line in bytes.split_inclusive(|v| *v == b'\n') {
            // A writer may still be appending, or may have died mid-record.
            if !line.ends_with(b"\n") {
                break;
            }
            let event: StoredRunEvent =
                serde_json::from_slice(line).context("执行记录包含无效 JSON")?;
            if let StoredRunEvent::Started {
                run: started,
                configuration: input,
            } = event
            {
                ensure!(run.is_none(), "执行记录包含重复开始事件");
                run = Some(*started);
                configuration = Some(input);
                continue;
            }
            let Some(run) = run.as_mut() else {
                continue;
            };
            match event {
                StoredRunEvent::Workspace {
                    path,
                    branch,
                    elapsed_ms,
                } => {
                    run.workspace_path = Some(path);
                    run.branch = branch;
                    run.preparation_ms = Some(elapsed_ms);
                }
                StoredRunEvent::AgentStarted { pid, command } => {
                    run.agent_pid = Some(pid);
                    run.agent_command = Some(command);
                    run.status = RunStatus::Running;
                }
                StoredRunEvent::Session {
                    session_id,
                    elapsed_ms,
                } => {
                    run.session_id = Some(session_id);
                    run.session_acquired_ms = Some(elapsed_ms);
                }
                StoredRunEvent::Finished {
                    at,
                    status,
                    exit_code,
                    message,
                    duration_ms,
                } => {
                    run.finished_at = Some(at);
                    run.status = status;
                    run.exit_code = exit_code;
                    run.message = message;
                    run.duration_ms = Some(duration_ms);
                }
                StoredRunEvent::OutputTruncated {} => {}
                StoredRunEvent::Started { .. } => unreachable!(),
            }
        }
        if let Some(run) = run.as_mut() {
            ensure!(
                run.id == run_id && run.task_id == task_id,
                "执行文件 ID 不匹配"
            );
            if !run.status.terminal() && !live {
                run.status = RunStatus::Interrupted;
                run.message = Some("执行进程已结束，未留下完成记录".into());
            }
        }
        Ok(run.map(|run| RunDetail {
            run,
            configuration: configuration.expect("started event contains configuration"),
            stdout_path: self
                .run_output_path(task_id, run_id, RunOutput::Stdio)
                .expect("validated IDs"),
            stderr_path: self
                .run_output_path(task_id, run_id, RunOutput::Stderr)
                .expect("validated IDs"),
        }))
    }

    pub fn runs(&self, task_id: &str, before: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let mut runs = Vec::new();
        if limit == 0 {
            return Ok(runs);
        }
        for id in self.run_ids(task_id, before)? {
            match self.read_run(task_id, &id) {
                Ok(Some(run)) => runs.push(run),
                Ok(None) => continue,
                Err(error) if is_not_found(&error) => continue,
                Err(error) => return Err(error),
            }
            if runs.len() >= limit.min(500) {
                break;
            }
        }
        Ok(runs)
    }

    pub(crate) fn run_ids(&self, task_id: &str, before: Option<&str>) -> Result<Vec<String>> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if file_type.is_dir() && !file_type.is_symlink() {
                let id = entry.file_name().to_string_lossy().into_owned();
                if valid_component(&id).is_ok()
                    && before.is_none_or(|before| compare_ids(&id, before).is_lt())
                {
                    ids.push(id);
                }
            }
        }
        ids.sort_unstable_by(|a, b| compare_ids(b, a));
        Ok(ids)
    }
}
