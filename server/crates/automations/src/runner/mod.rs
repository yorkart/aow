//! Server-side invocation policy and node submission. No agent or Git processes run here.
use crate::{Run, RunEvent, RunSource, RunStatus, Store, Task, store::new_run_id};
use anyhow::Result;
use chrono::Utc;
use std::time::Instant;
mod remote;
pub use remote::{execution_reference, reconcile};
pub fn initial_run(task: &Task, id: String, source: RunSource) -> Run {
    Run {
        id,
        task_id: task.id.clone(),
        task_revision: task.revision,
        task_name: task.input.name.clone(),
        agent: task.input.agent,
        source,
        variables: (source == RunSource::Manual).then(Default::default),
        status: RunStatus::Preparing,
        started_at: Utc::now(),
        finished_at: None,
        workspace_path: None,
        branch: None,
        session_id: None,
        agent_pid: None,
        agent_command: None,
        exit_code: None,
        message: None,
        preparation_ms: None,
        session_acquired_ms: None,
        duration_ms: None,
    }
}

pub async fn run(
    store: &Store,
    task_id: &str,
    id: Option<String>,
    source: RunSource,
) -> Result<RunStatus> {
    remote::run(store, task_id, id, source).await
}
