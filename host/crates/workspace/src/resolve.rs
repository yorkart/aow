//! Resolve a node worktree against registered project roots.
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

async fn git(cwd: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(output.status.success(), "Node Git lookup failed");
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub async fn resolve_registered_worktree(
    cwd: &Path,
    registered_roots: &[PathBuf],
) -> Result<PathBuf> {
    ensure!(
        cwd.is_absolute(),
        "Workspace path must be absolute on the node"
    );
    let cwd = tokio::fs::canonicalize(cwd).await?;
    let top = tokio::fs::canonicalize(git(&cwd, &["rev-parse", "--show-toplevel"]).await?).await?;
    ensure!(cwd == top, "Workspace path must be a worktree root");
    let common = tokio::fs::canonicalize(
        git(
            &cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await?,
    )
    .await?;
    for root in registered_roots {
        if !root.is_absolute() {
            continue;
        }
        if let Ok(value) = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
            && tokio::fs::canonicalize(value)
                .await
                .is_ok_and(|candidate| candidate == common)
        {
            return Ok(cwd);
        }
    }
    anyhow::bail!("Workspace is not owned by a registered project")
}
