//! Git process ownership on the execution node.
use anyhow::{Context, Result};
pub use aow_host_contract::workspace::GitCommandResult;
use std::{path::Path, process::Stdio, time::Duration};
struct Group(Option<u32>);
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
pub async fn command(cwd: &Path, args: &[String]) -> Result<GitCommandResult> {
    command_with_timeout(cwd, args, Duration::from_secs(120)).await
}
async fn command_with_timeout(
    cwd: &Path,
    args: &[String],
    deadline: Duration,
) -> Result<GitCommandResult> {
    anyhow::ensure!(cwd.is_absolute(), "Git cwd must be absolute on the node");
    let child = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false")
        .env("SSH_ASKPASS", "false")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let mut group = Group(child.id());
    let output = tokio::time::timeout(deadline, child.wait_with_output())
        .await
        .context("Node Git command timed out")??;
    group.0 = None;
    Ok(GitCommandResult {
        success: output.status.success(),
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}
#[derive(Default)]
pub struct NodeGit;
impl crate::GitExecutor for NodeGit {
    async fn output(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let result = command(
            cwd,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        )
        .await?;
        anyhow::ensure!(
            result.success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(String::from_utf8(result.stdout)?
            .trim_end_matches(['\r', '\n'])
            .to_owned())
    }
}

#[cfg(test)]
mod tests;
