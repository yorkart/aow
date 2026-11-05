#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::*;
use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

struct Fixture {
    child: Mutex<Child>,
    directory: tempfile::TempDir,
    exit_during_lookup: bool,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let child = self.child.get_mut().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let ready = root.join("ready");
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "runtime::session_titles::tests::native_environment_fixture",
            ])
            .env("AOW_NATIVE_ENV_FIXTURE", "1")
            .env("AOW_NATIVE_ENV_READY", &ready)
            .env("HOME", &root)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let fixture = Self {
            child: Mutex::new(child),
            directory,
            exit_during_lookup: false,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(fixture.child.lock().unwrap().try_wait().unwrap().is_none());
            assert!(
                Instant::now() < deadline,
                "native environment fixture timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fixture
    }

    fn process(&self) -> TerminalAgentProcess {
        let pid = self.child.lock().unwrap().id() as i32;
        TerminalAgentProcess {
            pid,
            start_time: aow_process::info(pid).unwrap().start_time,
            pi_binding: None,
            cwd: self
                .directory
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    }
}

impl AgentSessionTitleProvider for Fixture {
    async fn session_title(&self, context: LiveSessionContext<'_>) -> Option<String> {
        let mut child = self.child.lock().unwrap();
        assert_eq!(context.pid, Some(child.id() as i32));
        assert_eq!(
            context.environment["HOME"],
            self.directory.path().canonicalize().unwrap()
        );
        if self.exit_during_lookup {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        Some("Native session title".into())
    }
}

#[tokio::test]
async fn title_lookup_validates_process_identity_before_and_after_the_adapter() {
    let mut fixture = Fixture::new();
    let process = fixture.process();
    assert_eq!(
        native_title(&fixture, process.clone(), "").await.as_deref(),
        Some("Native session title")
    );
    let mut stale = process.clone();
    stale.start_time = "stale process".into();
    assert!(native_title(&fixture, stale, "").await.is_none());
    fixture.exit_during_lookup = true;
    assert!(native_title(&fixture, process.clone(), "").await.is_none());
    assert!(native_title(&fixture, process, "").await.is_none());
}

#[test]
#[ignore = "subprocess fixture invoked by node title tests"]
fn native_environment_fixture() {
    if std::env::var_os("AOW_NATIVE_ENV_FIXTURE").is_some() {
        if let Some(ready) = std::env::var_os("AOW_NATIVE_ENV_READY") {
            std::fs::write(ready, b"ready").unwrap();
        }
        std::io::stdin().read_line(&mut String::new()).unwrap();
    }
}

#[tokio::test]
#[ignore = "requires AOW_TITLE_TEST_PID and AOW_TITLE_TEST_EXPECTED; reads a live node process"]
async fn native_process_title_matches_the_live_session() {
    use aow_agent_runtime_core::process::{ProcessInfo, recognize_process};
    let pid = std::env::var("AOW_TITLE_TEST_PID")
        .unwrap()
        .parse()
        .unwrap();
    let expected = std::env::var("AOW_TITLE_TEST_EXPECTED").unwrap();
    let command = aow_process::command(pid);
    let args: Vec<_> = command
        .arguments
        .split(|byte| *byte == 0)
        .map(|arg| std::str::from_utf8(arg).unwrap_or(""))
        .collect();
    let agent = recognize_process(&ProcessInfo::new(
        command
            .executable
            .as_deref()
            .and_then(std::path::Path::to_str),
        &args,
    ))
    .unwrap();
    let process = TerminalAgentProcess {
        pid,
        start_time: aow_process::info(pid).unwrap().start_time,
        pi_binding: None,
        cwd: aow_process::cwd(pid)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    };
    let mut detected = TerminalAgentList {
        agents: [("live".into(), Some(agent.id().into()))].into(),
        processes: [("live".into(), process)].into(),
        ..Default::default()
    };
    enrich(&mut detected).await;
    assert_eq!(detected.titles["live"], expected);
}
