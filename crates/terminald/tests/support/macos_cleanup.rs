use super::*;
use std::os::unix::process::CommandExt;

const FIXTURE: &str = "macos_cleanup::session_cleanup_fixture";

struct Cleanup(Vec<(i32, String)>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for (pid, identity) in &self.0 {
            if aow_process::same_process(*pid, identity) {
                unsafe {
                    libc::kill(*pid, libc::SIGKILL);
                }
            }
        }
    }
}

fn fixture_command(directory: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            FIXTURE,
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AOW_CLEANUP_FIXTURE", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

async fn await_file(path: &std::path::Path) {
    timeout(IO_TIMEOUT, async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cleanup fixture did not start");
}

fn track(guard: &mut Cleanup, pid: i32) -> aow_process::ProcessInfo {
    let info = aow_process::info(pid).unwrap();
    guard.0.push((pid, info.start_time.clone()));
    info
}

#[tokio::test]
async fn cleanup_reaches_other_process_groups_without_touching_other_sessions() {
    let unrelated_dir = tempfile::tempdir().unwrap();
    let mut unrelated = fixture_command(unrelated_dir.path())
        .env("AOW_CLEANUP_CHILD", "1")
        .process_group(0)
        .spawn()
        .unwrap();
    let mut cleanup = Cleanup(Vec::new());
    let outside = track(&mut cleanup, unrelated.id() as i32);
    await_file(&unrelated_dir.path().join("ready")).await;

    for action in ["delete", "exit", "shutdown"] {
        let daemon = TestDaemon::start().await;
        let directory = tempfile::tempdir().unwrap();
        let mut spec = shell_spec(directory.path());
        spec.shell = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        spec.arguments = [
            "--ignored",
            "--exact",
            FIXTURE,
            "--nocapture",
            "--test-threads=1",
        ]
        .map(str::to_owned)
        .into();
        spec.environment.insert(
            "AOW_CLEANUP_FIXTURE".into(),
            directory.path().to_string_lossy().into_owned(),
        );
        daemon.client.create(action, &spec).await.unwrap();
        await_file(&directory.path().join("leader")).await;
        await_file(&directory.path().join("ready")).await;
        let read_pid = |name| {
            std::fs::read_to_string(directory.path().join(name))
                .unwrap()
                .parse::<i32>()
                .unwrap()
        };
        let leader = track(&mut cleanup, read_pid("leader"));
        let background = track(&mut cleanup, read_pid("child"));
        assert_eq!(background.session, leader.pid);
        assert_eq!(leader.session, leader.pid);
        assert_ne!(background.group, leader.group);
        assert_ne!(background.session, outside.session);

        match action {
            "delete" => {
                daemon.client.delete(action).await.unwrap();
                daemon.stop().await;
            }
            "exit" => {
                std::fs::write(directory.path().join("exit"), b"exit").unwrap();
                timeout(IO_TIMEOUT, async {
                    while daemon.client.get(action).await.unwrap().unwrap().status
                        != TerminalPaneStatus::Exited
                    {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                // Observe natural-exit cleanup before stopping the daemon.
                wait_until_process_gone(background.pid).await;
                daemon.stop().await;
            }
            "shutdown" => daemon.stop().await,
            _ => unreachable!(),
        }
        wait_until_process_gone(leader.pid).await;
        wait_until_process_gone(background.pid).await;
        assert!(
            unrelated.try_wait().unwrap().is_none(),
            "another session was killed"
        );
    }
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
}

#[test]
#[ignore = "subprocess fixture for macOS session cleanup"]
fn session_cleanup_fixture() {
    let Some(directory) = std::env::var_os("AOW_CLEANUP_FIXTURE") else {
        return;
    };
    let directory = PathBuf::from(directory);
    unsafe {
        libc::alarm(45);
    }
    if std::env::var_os("AOW_CLEANUP_CHILD").is_some() {
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
        std::fs::write(directory.join("ready"), b"ready").unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let child = fixture_command(&directory)
        .env("AOW_CLEANUP_CHILD", "1")
        .process_group(0)
        .spawn()
        .unwrap();
    std::fs::write(directory.join("child"), child.id().to_string()).unwrap();
    std::fs::write(directory.join("leader"), std::process::id().to_string()).unwrap();
    while !directory.join("exit").exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Intentionally leave the child for terminald's natural-exit cleanup.
}
