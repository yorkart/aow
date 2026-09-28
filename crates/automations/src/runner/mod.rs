mod agent;
mod process_group;
mod workspace;

use std::time::Instant;

use anyhow::Result;
use aow_agents::environment;
use chrono::Utc;

use crate::{Run, RunEvent, RunSource, RunStatus, Store, Task, store::new_run_id};

use self::workspace::{cleanup_failure, cleanup_worktree};

type Outcome = (RunStatus, Option<i32>, Option<String>);
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

fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub async fn run(
    store: &Store,
    task_id: &str,
    id: Option<String>,
    source: RunSource,
) -> Result<RunStatus> {
    let mut task = store.get_task(task_id)?;
    let deleted = task.deleted;
    let mut variables = Default::default();
    if source == RunSource::Manual {
        if let Some(request) = id
            .as_deref()
            .map(|id| store.take_manual_request(task_id, id))
            .transpose()?
            .flatten()
        {
            task = request.task;
            variables = request.variables;
        }
    }
    task.launch.environment.remove("PATH");
    let mut run = initial_run(&task, id.unwrap_or_else(new_run_id), source);
    if source == RunSource::Manual {
        run.variables = Some(variables.clone());
    }
    let started = Instant::now();
    let Some(mut concurrency_slot) =
        store.concurrency_slot(&task.id, task.input.max_concurrent_runs)?
    else {
        return Ok(RunStatus::Skipped);
    };
    concurrency_slot.begin(&run.id)?;
    let mut writer = store.create_run(&run, &task)?;
    let mut outcome = if deleted
        || task.deleted
        || (source == RunSource::Scheduled
            && (!task.input.enabled || task.input.kind == crate::TaskKind::Manual))
    {
        Ok((RunStatus::Skipped, None, Some("任务已暂停或删除".into())))
    } else {
        tokio::select! {
            result = async {
                // Keep the original template and bindings in history. Resolve
                // only this in-memory execution copy, without parsing variables.
                task.input.prompt = task.input.render_prompt(&variables)?;
                task.input.prompt_bindings.clear();
                let mut agent_environment = environment::load_agent_environment_from(
                    &store.config_dir,
                    task.input.agent.id(),
                ).await?;
                // PATH is always loaded from the shared execution settings, even
                // when an Agent registration contains its own PATH override.
                agent_environment.remove("PATH");
                let paths = environment::load_path_from(&store.config_dir).await?;
                task.launch.environment.insert("PATH".into(), environment::path_value(&paths)?);
                agent::execute(
                    store,
                    &task,
                    &agent_environment,
                    &run.id,
                    &mut writer,
                    started,
                    &concurrency_slot,
                ).await
            } => result,
            _ = shutdown_signal() => Ok((RunStatus::Interrupted, None, Some("执行收到停止信号".into()))),
        }
    };
    if let Err(error) = cleanup_worktree(store, &task, &run.id, &concurrency_slot).await {
        outcome = cleanup_failure(outcome, error);
    }
    let (status, exit_code, message) = match outcome {
        Ok(outcome) => outcome,
        Err(error) => (
            RunStatus::Failed,
            None,
            Some(format!("{error:#}").chars().take(2000).collect()),
        ),
    };
    writer.append(&RunEvent::Finished {
        at: Utc::now(),
        status,
        exit_code,
        message,
        duration_ms: millis(started),
    })?;
    Ok(status)
}
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    if let Ok(mut terminate) = signal(SignalKind::terminate()) {
        tokio::select! { _ = terminate.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}
