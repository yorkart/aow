use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use tempfile::TempDir;
use tokio::process::Command;

use crate::{
    RunStatus, Store, Task, WorkspaceMode, store::private_dir, task_lock::ConcurrencySlot,
};

use super::{Outcome, process_group::ProcessGroup};

pub(super) struct PreparedWorkspace {
    pub(super) directory: PathBuf,
    pub(super) branch: Option<String>,
    // Keeping TempDir alive makes the directory available to the Agent for the
    // full run, then removes it on every normal return or cancellation path.
    _temporary_directory: Option<TempDir>,
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
        .context("Git 操作超时")??;
    ensure!(
        output.status.success(),
        "Git 操作失败: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1000)
            .collect::<String>()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
pub(super) async fn prepare(
    store: &Store,
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<PreparedWorkspace> {
    if task.input.workspace_mode == WorkspaceMode::Temporary {
        let temporary = tempfile::Builder::new()
            .prefix(&format!("aow-automation-{}-{run_id}-", task.id))
            .tempdir()
            .context("无法创建临时工作区")?;
        return Ok(PreparedWorkspace {
            directory: temporary.path().to_path_buf(),
            branch: None,
            _temporary_directory: Some(temporary),
        });
    }
    let root = task
        .repository_path
        .canonicalize()
        .context("项目目录不存在")?;
    let requested = task
        .input
        .workspace_path
        .canonicalize()
        .context("工作区目录不存在")?;
    let root_common = git(
        task,
        &root,
        &["rev-parse", "--git-common-dir"],
        concurrency_slot,
    )
    .await?;
    let worktree_common = git(
        task,
        &requested,
        &["rev-parse", "--git-common-dir"],
        concurrency_slot,
    )
    .await?;
    ensure!(
        root.join(root_common).canonicalize()? == requested.join(worktree_common).canonicalize()?,
        "工作区不属于该项目"
    );
    let directory = match task.input.workspace_mode {
        WorkspaceMode::NewWorktree => {
            let directory = store.root.join("worktrees").join(&task.id).join(run_id);
            private_dir(directory.parent().unwrap())?;
            let branch = format!("automation/{}/{}", task.id, run_id);
            let base = git(
                task,
                &root,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", task.input.base_branch),
                ],
                concurrency_slot,
            )
            .await?;
            git(
                task,
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &branch,
                    directory.to_str().context("工作区路径不是 UTF-8")?,
                    &base,
                ],
                concurrency_slot,
            )
            .await?;
            directory
        }
        WorkspaceMode::Existing | WorkspaceMode::NewBranch => {
            if task.input.workspace_mode == WorkspaceMode::NewBranch {
                ensure!(
                    git(
                        task,
                        &requested,
                        &["status", "--porcelain"],
                        concurrency_slot
                    )
                    .await?
                    .is_empty(),
                    "工作区有未提交修改，无法创建并切换分支"
                );
                let branch = format!("automation/{}/{}", task.id, run_id);
                let base = git(
                    task,
                    &root,
                    &[
                        "rev-parse",
                        "--verify",
                        &format!("{}^{{commit}}", task.input.base_branch),
                    ],
                    concurrency_slot,
                )
                .await?;
                git(
                    task,
                    &requested,
                    &["checkout", "-b", &branch, &base],
                    concurrency_slot,
                )
                .await?;
            }
            requested
        }
        WorkspaceMode::Temporary => unreachable!("temporary workspaces return before Git setup"),
    };
    let branch = git(
        task,
        &directory,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        concurrency_slot,
    )
    .await?;
    Ok(PreparedWorkspace {
        directory,
        branch: Some(branch),
        _temporary_directory: None,
    })
}
pub(super) fn cleanup_failure(outcome: Result<Outcome>, error: anyhow::Error) -> Result<Outcome> {
    let cleanup = format!("清理 Worktree 失败: {error:#}");
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
    store: &Store,
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<()> {
    if task.input.workspace_mode != WorkspaceMode::NewWorktree {
        return Ok(());
    }
    let directory = store.root.join("worktrees").join(&task.id).join(run_id);
    if !directory.exists() {
        return Ok(());
    }
    let root = task
        .repository_path
        .canonicalize()
        .context("项目目录不存在，无法清理 Worktree")?;
    git(
        task,
        &root,
        &[
            "worktree",
            "remove",
            "--force",
            directory.to_str().context("工作区路径不是 UTF-8")?,
        ],
        concurrency_slot,
    )
    .await?;
    Ok(())
}
