use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use tokio::process::Command;

use super::{Outcome, process_group::ProcessGroup};
use crate::{RunStatus, Task, WorkspaceMode, task_lock::ConcurrencySlot};

pub(super) struct PreparedWorkspace {
    pub(super) directory: PathBuf,
    pub(super) branch: Option<String>,
    // Cleanup belongs to Automation, including early returns and cancellation.
    _temporary_cleanup: Option<TemporaryCleanup>,
}

struct TemporaryCleanup(PathBuf);
impl Drop for TemporaryCleanup {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to clean up temporary workspace {}: {error}",
                self.0.display()
            );
        }
    }
}

struct AutomationGit<'a> {
    task: &'a Task,
    slot: &'a ConcurrencySlot,
}

impl aow_workspaces::GitExecutor for AutomationGit<'_> {
    async fn output(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        git(self.task, cwd, args, self.slot).await
    }
}

async fn git(task: &Task, cwd: &Path, args: &[&str], slot: &ConcurrencySlot) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .envs(&task.launch.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    slot.register(&mut command);
    let child = command.spawn()?;
    let _group = ProcessGroup(child.id().unwrap());
    let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .context(aow_i18n::text("automations.git_operation_timed_out"))??;
    ensure!(
        output.status.success(),
        aow_i18n::message(
            "automations.git_operation_failed",
            &[(
                "auto0",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(1000)
                    .collect::<String>()
                    .to_string()
            )]
        )
    );
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}

pub(super) async fn prepare(
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<PreparedWorkspace> {
    let executor = AutomationGit {
        task,
        slot: concurrency_slot,
    };
    let workspace = aow_workspaces::prepare(
        &task.input.workspace,
        &task.repository_path,
        &format!("automation/{}/{run_id}", task.id),
        &executor,
    )
    .await?;
    let cleanup = (workspace.workspace_mode == WorkspaceMode::Temporary)
        .then(|| TemporaryCleanup(workspace.directory.clone()));
    Ok(PreparedWorkspace {
        directory: workspace.directory,
        branch: workspace.branch,
        _temporary_cleanup: cleanup,
    })
}
pub(super) fn cleanup_failure(outcome: Result<Outcome>, error: anyhow::Error) -> Result<Outcome> {
    let cleanup = aow_i18n::message(
        "automations.failed_to_clean_up_worktree",
        &[("p0", format!("{:#}", error))],
    );
    match outcome {
        Ok((_, exit_code, message)) => Ok((
            RunStatus::Failed,
            exit_code,
            Some(match message {
                Some(message) => format!("{message}; {cleanup}"),
                None => cleanup,
            }),
        )),
        Err(error) => Err(anyhow::anyhow!("{error:#}; {cleanup}")),
    }
}
pub(super) async fn cleanup_worktree(
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<()> {
    if task.input.workspace.workspace_mode != WorkspaceMode::NewWorktree {
        return Ok(());
    }
    let executor = AutomationGit {
        task,
        slot: concurrency_slot,
    };
    let directory = aow_workspaces::worktree_directory(
        &task.repository_path,
        &format!("automation/{}/{run_id}", task.id),
        &executor,
    )
    .await?;
    if !directory.exists() {
        return Ok(());
    }
    let root = task.repository_path.canonicalize().context(aow_i18n::text(
        "automations.project_directory_missing_cannot_clean_up_worktree",
    ))?;
    git(
        task,
        &root,
        &[
            "worktree",
            "remove",
            "--force",
            directory
                .to_str()
                .context(aow_i18n::text("automations.workspace_path_is_not_utf_8"))?,
        ],
        concurrency_slot,
    )
    .await?;
    Ok(())
}
