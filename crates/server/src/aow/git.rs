use std::{
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};

use tokio::{process::Command, time::timeout};

use super::AowError;

// Allow time for network transfers and checkout filters, but never hold the
// project's worktree lock indefinitely.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

struct GitProcessGroup(Option<u32>);

impl Drop for GitProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // Git can spawn credential helpers, transports and hooks. Killing
            // only Git lets these keep running after a timeout or cancellation.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

async fn command_output(cwd: &Path, args: &[&str], deadline: Duration) -> Result<Output, AowError> {
    let child = Command::new("git")
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
        .spawn()
        .map_err(|error| AowError::Git(error.to_string()))?;
    let mut group = GitProcessGroup(child.id());
    let output = timeout(deadline, child.wait_with_output())
        .await
        .map_err(|_| {
            AowError::Git(format!(
                "git {} timed out after {} seconds in {}",
                args.join(" "),
                deadline.as_secs(),
                cwd.display()
            ))
        })?
        .map_err(|error| AowError::Git(error.to_string()))?;
    // A completed command may intentionally leave a background hook running.
    group.0 = None;
    Ok(output)
}

fn command_error(args: &[&str], output: &Output) -> AowError {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    AowError::Git(if message.is_empty() {
        format!("git {} exited with {}", args.join(" "), output.status)
    } else {
        message
    })
}

pub(super) async fn git_output(cwd: &Path, args: &[&str]) -> Result<String, AowError> {
    let output = command_output(cwd, args, COMMAND_TIMEOUT).await?;
    if !output.status.success() {
        return Err(command_error(args, &output));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| AowError::Git(format!("git output is not UTF-8: {error}")))
}

pub(super) async fn optional_git_output(
    cwd: &Path,
    args: &[&str],
) -> Result<Option<String>, AowError> {
    let output = command_output(cwd, args, COMMAND_TIMEOUT).await?;
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|error| AowError::Git(format!("git output is not UTF-8: {error}")));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    Err(command_error(args, &output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn hanging_git_is_stopped(cancel: bool) {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_path_buf();
        git_output(&cwd, &["init", "-q"]).await.unwrap();
        let marker = cwd.join("pids");
        let task = tokio::spawn(async move {
            command_output(
                &cwd,
                &[
                    "-c",
                    "alias.aow-hang=!sleep 30 & child=$!; printf '%s\\n' \"$$\" \"$child\" > pids; wait",
                    "aow-hang",
                ],
                Duration::from_secs(2),
            )
            .await
        });
        let pids = timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(value) = std::fs::read_to_string(&marker) {
                    let pids: Vec<i32> = value.lines().filter_map(|pid| pid.parse().ok()).collect();
                    if pids.len() == 2 {
                        break pids;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Git alias did not start");

        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            let error = task.await.unwrap().unwrap_err().to_string();
            assert!(error.contains("timed out after 2 seconds"), "{error}");
            assert!(error.contains("aow-hang"), "{error}");
        }

        timeout(Duration::from_secs(3), async {
            loop {
                if pids.iter().all(|pid| match aow_process::info(*pid) {
                    Ok(process) => process.state == 'Z',
                    Err(error) => {
                        assert!(
                            error.kind() == std::io::ErrorKind::NotFound
                                || error.raw_os_error() == Some(libc::ESRCH),
                            "cannot inspect child {pid}: {error}"
                        );
                        true
                    }
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Git helper survived the request");
    }

    #[tokio::test]
    async fn timeout_stops_git_and_its_descendants() {
        hanging_git_is_stopped(false).await;
    }

    #[tokio::test]
    async fn cancellation_stops_git_and_its_descendants() {
        hanging_git_is_stopped(true).await;
    }

    #[tokio::test]
    async fn authentication_failure_does_not_open_an_askpass_prompt() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let prompt = directory.path().join("askpass");
        let marker = directory.path().join("prompt-opened");
        std::fs::write(&prompt, "#!/bin/sh\ntouch prompt-opened\nsleep 30\n").unwrap();
        std::fs::set_permissions(&prompt, std::fs::Permissions::from_mode(0o700)).unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/repo.git", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            }
        });
        let output = command_output(
            directory.path(),
            &[
                "-c",
                "credential.helper=",
                "-c",
                "http.proxy=",
                "-c",
                &format!("core.askPass={}", prompt.display()),
                "ls-remote",
                &url,
            ],
            Duration::from_secs(3),
        )
        .await;
        server.abort();
        let output = output.expect("authentication should fail without waiting for a prompt");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("terminal prompts disabled"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!marker.exists());
    }
}
