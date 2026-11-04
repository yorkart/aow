//! Node-owned interactive readiness and prompt delivery. Retries reuse an immutable operation ID.
use super::*;
use aow_agents::interactive::AgentInteractive;
use aow_runtime_contract::agents::{
    TerminalAgentOperation, TerminalAgentProgress, TerminalAgentRequest,
};
pub(super) struct Record {
    fingerprint: md5::Digest,
    progress: TerminalAgentProgress,
}
fn runtime(state: &DaemonState, id: &str) -> Result<Arc<Runtime>, TerminaldError> {
    state
        .lock_runtimes()?
        .get(id)
        .filter(|runtime| !runtime.is_deleted())
        .cloned()
        .ok_or_else(|| TerminaldError::NotFound(id.into()))
}
pub(super) async fn status(
    State(state): State<DaemonState>,
    AxumPath((id, request_id)): AxumPath<(String, String)>,
) -> Result<Json<TerminalAgentProgress>, TerminaldError> {
    let runtime = runtime(&state, &id)?;
    let records = runtime
        .agent_operations
        .lock()
        .map_err(|_| TerminaldError::Poisoned)?;
    records
        .get(&request_id)
        .map(|r| Json(r.progress.clone()))
        .ok_or(TerminaldError::NotFound(request_id))
}
pub(super) async fn admit(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<TerminalAgentRequest>,
) -> Result<Json<TerminalAgentProgress>, TerminaldError> {
    if state.inner.shutting_down.load(Ordering::Acquire) {
        return Err(TerminaldError::Conflict("Runtime is stopping".into()));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > 128
        || !request
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(TerminaldError::Invalid("Invalid operation ID".into()));
    }
    let (task, timeout_seconds) = match &request.operation {
        TerminalAgentOperation::Initialize {
            agent,
            task,
            timeout_seconds,
        } => {
            if aow_agents::Agent::from_id(agent)
                .and_then(aow_agents::Agent::interactive)
                .is_none()
            {
                return Err(TerminaldError::Invalid(
                    "Agent does not support interactive startup".into(),
                ));
            }
            (task.as_deref(), *timeout_seconds)
        }
        TerminalAgentOperation::Submit { task } => (Some(task.as_str()), 15),
    };
    if !(1..=600).contains(&timeout_seconds) {
        return Err(TerminaldError::Invalid(
            "Invalid agent operation timeout".into(),
        ));
    }
    if let Some(task) = task {
        validate_task(task)?;
    }
    let fingerprint = md5::compute(
        serde_json::to_vec(&request.operation)
            .map_err(|error| TerminaldError::Invalid(error.to_string()))?,
    );
    let runtime = runtime(&state, &id)?;
    let progress = TerminalAgentProgress {
        request_id: request.request_id.clone(),
        finished: false,
        task_submitted: false,
        error: None,
    };
    {
        let mut records = runtime
            .agent_operations
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if let Some(record) = records.get(&request.request_id) {
            if record.fingerprint != fingerprint {
                return Err(TerminaldError::Conflict(
                    "Operation ID already has another request".into(),
                ));
            }
            return Ok(Json(record.progress.clone()));
        }
        if records.len() >= 65_536 || records.values().any(|record| !record.progress.finished) {
            return Err(TerminaldError::Conflict(
                "Agent operation is busy or its history is full".into(),
            ));
        }
        records.insert(
            request.request_id.clone(),
            Record {
                fingerprint,
                progress: progress.clone(),
            },
        );
    }
    tokio::spawn(async move {
        let result = timeout(
            Duration::from_secs(timeout_seconds),
            execute(&state, runtime.clone(), &request.operation),
        )
        .await;
        let mut records = runtime.agent_operations.lock().unwrap();
        let record = records.get_mut(&request.request_id).unwrap();
        record.progress.finished = true;
        match result {
            Ok(Ok(submitted)) => record.progress.task_submitted = submitted,
            Ok(Err(error)) => record.progress.error = Some(error.to_string()),
            Err(_) => {
                record.progress.error =
                    Some("Agent operation timed out; delivery may be unknown".into())
            }
        }
    });
    Ok(Json(progress))
}
struct Lease {
    runtime: Arc<Runtime>,
    owner: ControllerOwner,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.runtime.release(self.owner);
    }
}
async fn write(
    runtime: &Arc<Runtime>,
    owner: ControllerOwner,
    data: String,
) -> Result<(), TerminaldError> {
    let _gate = runtime.delivery_gate.lock().await;
    let runtime = runtime.clone();
    let written = tokio::task::spawn_blocking(move || runtime.write_input(owner, data.as_bytes()))
        .await
        .map_err(|e| TerminaldError::Worker(e.to_string()))??;
    if !written {
        return Err(TerminaldError::Conflict("Terminal control was lost".into()));
    }
    Ok(())
}
async fn execute(
    state: &DaemonState,
    runtime: Arc<Runtime>,
    operation: &TerminalAgentOperation,
) -> Result<bool, TerminaldError> {
    let attachment_id = u128::from(aow_id::new_snowflake().as_u64());
    let owner = match runtime.claim(attachment_id, None, false, false, false)? {
        ClaimOutcome::Claimed(claimed) => claimed.owner,
        _ => {
            return Err(TerminaldError::Conflict(
                "Pane is controlled by another client".into(),
            ));
        }
    };
    let lease = Lease { runtime, owner };
    let task = match operation {
        TerminalAgentOperation::Initialize { agent, task, .. } => {
            let adapter = aow_agents::Agent::from_id(agent)
                .and_then(aow_agents::Agent::interactive)
                .unwrap();
            loop {
                if state.inner.shutting_down.load(Ordering::Acquire)
                    || lease.runtime.is_deleted()
                    || lease.runtime.description()?.status != TerminalPaneStatus::Running
                {
                    return Err(TerminaldError::Conflict(
                        "Agent exited before becoming ready".into(),
                    ));
                }
                if !lease.runtime.is_owner(owner)? {
                    return Err(TerminaldError::Conflict("Terminal control was lost".into()));
                }
                if state
                    .screen(&lease.runtime.id)?
                    .is_some_and(|screen| adapter.input_ready(&screen.lines))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            task.as_deref()
        }
        TerminalAgentOperation::Submit { task } => Some(task.as_str()),
    };
    if let Some(task) = task {
        submit_owned(&lease.runtime, owner, task).await?;
    }
    Ok(task.is_some())
}

pub(in crate::runtime) fn validate_task(task: &str) -> Result<(), TerminaldError> {
    if task.trim().is_empty()
        || task.len() > 128 * 1024
        || task
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(TerminaldError::Invalid("Invalid task text".into()));
    }
    Ok(())
}
pub(in crate::runtime) async fn submit_owned(
    runtime: &Arc<Runtime>,
    owner: ControllerOwner,
    task: &str,
) -> Result<(), TerminaldError> {
    validate_task(task)?;
    if runtime.is_deleted() || runtime.description()?.status != TerminalPaneStatus::Running {
        return Err(TerminaldError::Conflict("Agent exited".into()));
    }
    write(runtime, owner, format!("\x1b[200~{task}\x1b[201~")).await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    write(runtime, owner, "\r".into()).await
}
