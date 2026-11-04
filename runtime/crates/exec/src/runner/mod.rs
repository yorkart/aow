mod agent;
mod process_group;
mod result;
mod workspace;
use crate::{RunEvent, RunStatus, Store, Task, task_lock::ConcurrencySlot};
use anyhow::Result;
use std::time::Instant;
type Outcome = (RunStatus, Option<i32>, Option<String>);
fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}
pub(crate) async fn execute(
    store: Store,
    mut task: Task,
    mut writer: crate::store::RunWriter,
    mut cancelled: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let started = Instant::now();
    let slot = ConcurrencySlot::open(&store.run_path("", &task.id)?.join("owner.lock"))?;
    let paths = aow_agents::environment::load_path_from(&store.config_dir).await?;
    task.launch
        .environment
        .entry("AOW_STATE_DIR".into())
        .or_insert_with(|| store.config_dir.to_string_lossy().into_owned());
    task.launch.environment.remove("PATH");
    task.agent_environment.remove("PATH");
    task.launch
        .environment
        .insert("PATH".into(), aow_agents::environment::path_value(&paths)?);
    let result = tokio::select! {
        result=agent::execute(&store,&task,&task.agent_environment,&task.id,&mut writer,started,&slot)=>result,
        _=cancelled.changed()=>Ok((RunStatus::Interrupted,None,Some("Execution was cancelled".into()))),
    };
    let result = match workspace::cleanup_worktree(&task, &task.id, &slot).await {
        Ok(()) => result,
        Err(error) => workspace::cleanup_failure(result, error),
    };
    let (status, exit_code, message) = match result {
        Ok(outcome) => outcome,
        Err(error) => (RunStatus::Failed, None, Some(format!("{error:#}"))),
    };
    writer.append(&RunEvent::Finished {
        at: chrono::Utc::now(),
        status,
        exit_code,
        message,
        duration_ms: millis(started),
    })?;
    Ok(())
}
