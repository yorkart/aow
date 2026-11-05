use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

async fn hanging_git_is_stopped(cancel: bool) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_path_buf();
    command(&cwd, &["init".into(), "-q".into()]).await.unwrap();
    let marker = cwd.join("pids");
    let task = aow_i18n::spawn(async move {
        command_with_timeout(
                &cwd,
                &[
                    "-c".into(),
                    "alias.aow-hang=!sleep 30 & child=$!; printf '%s\\n' \"$$\" \"$child\" > pids; wait".into(),
                    "aow-hang".into(),
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
        assert!(error.contains("timed out"), "{error}");
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
    let server = aow_i18n::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await;
            let _ = stream
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
        }
    });
    let output = command_with_timeout(
        directory.path(),
        &[
            "-c".into(),
            "credential.helper=".into(),
            "-c".into(),
            "http.proxy=".into(),
            "-c".into(),
            format!("core.askPass={}", prompt.display()),
            "ls-remote".into(),
            url,
        ],
        Duration::from_secs(3),
    )
    .await;
    server.abort();
    let output = output.expect("authentication should fail without waiting for a prompt");
    assert!(!output.success);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("terminal prompts disabled"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists());
}
