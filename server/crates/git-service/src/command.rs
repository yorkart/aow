use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use super::GitError;

pub(super) const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const SMALL_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const DIFF_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
pub(super) struct LimitedOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

pub(super) async fn run_git(
    repository: &Path,
    args: &[&str],
    limit: usize,
) -> Result<LimitedOutput, GitError> {
    run_git_with_environment(repository, args, limit, &[]).await
}

pub(super) async fn run_git_with_environment(
    repository: &Path,
    args: &[&str],
    limit: usize,
    environment: &[(&str, &str)],
) -> Result<LimitedOutput, GitError> {
    run_git_with_timeout(repository, args, limit, environment, COMMAND_TIMEOUT).await
}

pub(super) async fn run_git_with_timeout(
    repository: &Path,
    args: &[&str],
    limit: usize,
    environment: &[(&str, &str)],
    command_timeout: Duration,
) -> Result<LimitedOutput, GitError> {
    let future = async {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout configured");
        let stderr = child.stderr.take().expect("stderr configured");
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(256 * 1024).read_to_end(&mut bytes).await;
            bytes
        });
        let mut bytes = Vec::with_capacity(limit.min(256 * 1024));
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut truncated = false;
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(bytes.len());
            let take = count.min(remaining);
            bytes.extend_from_slice(&buffer[..take]);
            if take < count || bytes.len() >= limit {
                truncated = true;
                child.kill().await?;
                break;
            }
        }
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !status.success() && !truncated {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(GitError::Command(if message.is_empty() {
                format!("git exited with {status}")
            } else {
                message
            }));
        }
        Ok(LimitedOutput { bytes, truncated })
    };

    timeout(command_timeout, future)
        .await
        .map_err(|_| GitError::Timeout)?
}

pub(super) async fn run_git_check_ignore(
    repository: &Path,
    input: Vec<u8>,
) -> Result<LimitedOutput, GitError> {
    let future = async {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["check-ignore", "-z", "--stdin"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = child.stdin.take().expect("stdin configured");
        let stdin_task = tokio::spawn(async move { stdin.write_all(&input).await });
        let mut stdout = child.stdout.take().expect("stdout configured");
        let stderr = child.stderr.take().expect("stderr configured");
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(256 * 1024).read_to_end(&mut bytes).await;
            bytes
        });
        let mut bytes = Vec::with_capacity(64 * 1024);
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut truncated = false;
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let remaining = SMALL_OUTPUT_LIMIT.saturating_sub(bytes.len());
            let take = count.min(remaining);
            bytes.extend_from_slice(&buffer[..take]);
            if take < count || bytes.len() >= SMALL_OUTPUT_LIMIT {
                truncated = true;
                child.kill().await?;
                break;
            }
        }
        stdin_task
            .await
            .map_err(|error| GitError::Command(error.to_string()))??;
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !matches!(status.code(), Some(0 | 1)) && !truncated {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(GitError::Command(if message.is_empty() {
                format!("git exited with {status}")
            } else {
                message
            }));
        }
        Ok(LimitedOutput { bytes, truncated })
    };

    timeout(COMMAND_TIMEOUT, future)
        .await
        .map_err(|_| GitError::Timeout)?
}
pub(super) fn absolute(path: impl AsRef<Path>) -> Result<PathBuf, GitError> {
    let path = path.as_ref();
    if !path.is_absolute() {
        return Err(GitError::PathNotAbsolute(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(path.to_path_buf())
}
