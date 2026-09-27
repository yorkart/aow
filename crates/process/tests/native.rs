#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::{
    io::{BufRead, Write},
    time::{Duration, Instant},
};

use aow_process::{command, cwd, environment, info, list_pids, same_process};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

struct ChildGuard(Box<dyn Child + Send + Sync>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "test process did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn inspects_native_pty_identity_arguments_environment_and_changing_cwd() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let first = root.join("first directory with spaces");
    let second = root.join("second directory with spaces");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let ready = root.join("ready");
    let moved = root.join("moved");
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    // A locally built executable exercises the same environment visibility as
    // ordinary CLI tools. SIP-protected /bin programs deliberately redact it.
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut builder = CommandBuilder::new(&executable);
    builder.args([
        "--ignored",
        "--exact",
        "native_process_fixture",
        "--nocapture",
        "--test-threads=1",
        "--skip",
        "unused filter with spaces",
    ]);
    builder.cwd(&first);
    builder.env("AOW_READY", &ready);
    builder.env("AOW_NEXT", &second);
    builder.env("AOW_MOVED", &moved);
    builder.env("AOW_PROCESS_TEST", "value with spaces=equals");
    let mut child = ChildGuard(pair.slave.spawn_command(builder).unwrap());
    drop(pair.slave);
    let pid = child.0.process_id().unwrap() as i32;
    wait_for(|| ready.exists());

    let process = info(pid).unwrap();
    assert_eq!(
        (
            process.pid,
            process.session,
            process.group,
            process.foreground
        ),
        (pid, pid, pid, pid)
    );
    assert_eq!(process.parent, std::process::id() as i32);
    assert_ne!(process.tty, 0);
    assert_eq!(pair.master.process_group_leader(), Some(pid));
    assert!(list_pids().unwrap().contains(&pid));
    assert_eq!(cwd(pid).unwrap(), first);
    assert!(same_process(pid, &process.start_time));
    assert!(!same_process(pid, "different start identity"));
    assert!(environment(pid, "different start identity").is_err());

    let command = command(pid);
    assert_eq!(command.executable.unwrap(), executable);
    let arguments: Vec<_> = command.arguments.split(|byte| *byte == 0).collect();
    assert_eq!(arguments[7], b"unused filter with spaces");
    let env = environment(pid, &process.start_time).unwrap();
    assert!(
        env.split(|byte| *byte == 0)
            .any(|entry| entry == b"AOW_PROCESS_TEST=value with spaces=equals")
    );

    let mut writer = pair.master.take_writer().unwrap();
    writer.write_all(b"move\n").unwrap();
    wait_for(|| moved.exists());
    assert_eq!(cwd(pid).unwrap(), second);
    assert!(same_process(pid, &process.start_time));

    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
    wait_for(|| info(pid).is_ok_and(|info| info.state == 'T'));
    assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
    wait_for(|| info(pid).is_ok_and(|info| info.state != 'T'));

    writer.write_all(b"finish\n").unwrap();
    wait_for(|| child.0.try_wait().unwrap().is_some());
    assert!(!same_process(pid, &process.start_time));
    assert!(environment(pid, &process.start_time).is_err());
    assert!(info(-1).is_err());
    assert!(!same_process(0, &process.start_time));
}

#[test]
#[ignore = "subprocess fixture invoked by the native PTY test"]
fn native_process_fixture() {
    let Some(ready) = std::env::var_os("AOW_READY") else {
        return;
    };
    std::fs::write(ready, b"ready").unwrap();
    let mut lines = std::io::stdin().lock().lines();
    lines.next().unwrap().unwrap();
    std::env::set_current_dir(std::env::var_os("AOW_NEXT").unwrap()).unwrap();
    std::fs::write(std::env::var_os("AOW_MOVED").unwrap(), b"moved").unwrap();
    lines.next().unwrap().unwrap();
}
