use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::PathBuf};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde::Deserialize;

use crate::{Run, RunDetail, RunOutput, RunStatus, TaskInput};

use super::super::{FileLock, Store, is_not_found, try_shared};

pub(super) const MAX_RUN_EVENTS_BYTES: u64 = 1024 * 1024;

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
            aow_i18n::message("automations.run_record_exceeds_size_limit", &[])
        );
        ensure!(
            file.metadata()?.is_file(),
            aow_i18n::message("automations.run_record_is_not_a_regular_file", &[])
        );
        let mut bytes = Vec::new();
        (&mut *file)
            .take(MAX_RUN_EVENTS_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_RUN_EVENTS_BYTES,
            aow_i18n::message("automations.run_record_exceeds_size_limit", &[])
        );
        let mut run = None;
        let mut configuration = None;
        for line in bytes.split_inclusive(|v| *v == b'\n') {
            // A writer may still be appending, or may have died mid-record.
            if !line.ends_with(b"\n") {
                break;
            }
            let event: StoredRunEvent = serde_json::from_slice(line).context(aow_i18n::text(
                "automations.run_record_contains_invalid_json",
            ))?;
            if let StoredRunEvent::Started {
                run: started,
                configuration: input,
            } = event
            {
                ensure!(
                    run.is_none(),
                    aow_i18n::message(
                        "automations.run_record_contains_duplicate_start_events",
                        &[]
                    )
                );
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
                aow_i18n::message("automations.run_file_id_mismatch", &[])
            );
            if !run.status.terminal() && !live {
                run.status = RunStatus::Interrupted;
                run.message = Some(
                    aow_i18n::text(
                        "automations.execution_process_ended_without_a_completion_record",
                    )
                    .into(),
                );
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
}
