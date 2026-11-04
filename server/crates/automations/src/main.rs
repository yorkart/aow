use anyhow::{Context, Result, bail};
use aow_automations::{RunSource, RunStatus, Scheduler, Store};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(action) = arguments.next() else {
        help();
        return Ok(());
    };
    if action == "--help" || action == "-h" {
        help();
        return Ok(());
    }
    let mut state_dir = None;
    let mut task_id = None;
    let mut run_id = None;
    let mut source = RunSource::Scheduled;
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .with_context(|| format!("{argument} requires a value"))?;
        match argument.as_str() {
            "--state-dir" => state_dir = Some(PathBuf::from(value)),
            "--task-id" => task_id = Some(value),
            "--run-id" => run_id = Some(value),
            "--source" => {
                source = match value.as_str() {
                    "scheduled" => RunSource::Scheduled,
                    "manual" => RunSource::Manual,
                    _ => bail!("unknown source"),
                }
            }
            _ => bail!("unknown option {argument}"),
        }
    }
    let store = Store::new(state_dir.context("--state-dir is required")?)?;
    match action.as_str() {
        "run" => {
            let task_id = task_id.context("--task-id is required")?;
            let status = aow_automations::runner::run(&store, &task_id, run_id, source).await?;
            if matches!(status, RunStatus::Failed | RunStatus::Interrupted) {
                std::process::exit(1);
            }
        }
        "trigger" => {
            let task_id = task_id.context("--task-id is required")?;
            let task = store.get_task(&task_id)?;
            if task.deleted
                || (source == RunSource::Scheduled
                    && (!task.input.enabled
                        || task.input.kind == aow_automations::TaskKind::Manual))
            {
                return Ok(());
            }
            let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
            let mut scheduler = Scheduler::new(&home);
            scheduler.runner = std::env::current_exe()?;
            scheduler.dispatch(&store, &task, source).await?;
        }
        _ => bail!("unknown command {action}"),
    }
    Ok(())
}

fn help() {
    println!(
        "aow-automation-runner <run|trigger> --state-dir PATH --task-id ID [--run-id ID] [--source scheduled|manual]"
    );
}
