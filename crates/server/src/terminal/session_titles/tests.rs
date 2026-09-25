#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::*;
use rusqlite::{Connection, params};
use serde_json::json;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    children: Vec<ChildGuard>,
    connection: Connection,
    root: PathBuf,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let connection = Connection::open(root.join("state.db")).unwrap();
        connection
            .execute_batch(
                "
            PRAGMA journal_mode=WAL;
            CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT,
                started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT);
            CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT, role TEXT,
                content TEXT, timestamp REAL);
        ",
            )
            .unwrap();
        std::fs::create_dir(root.join("runtime")).unwrap();
        Self {
            children: Vec::new(),
            connection,
            root,
            _directory: directory,
        }
    }

    fn process(&mut self) -> TerminalAgentProcess {
        let ready = self.root.join(format!("ready-{}", self.children.len()));
        let child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "terminal::sessions::tests::native_environment_fixture",
                ])
                .env("AOW_NATIVE_ENV_FIXTURE", "1")
                .env("AOW_NATIVE_ENV_READY", &ready)
                .env("HOME", &self.root)
                .env("HERMES_HOME", &self.root)
                .current_dir(&self.root)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id() as i32;
        self.children.push(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                self.children
                    .last_mut()
                    .unwrap()
                    .0
                    .try_wait()
                    .unwrap()
                    .is_none()
            );
            assert!(
                Instant::now() < deadline,
                "native environment fixture timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        TerminalAgentProcess {
            pid,
            start_time: aow_process::info(pid).unwrap().start_time,
            cwd: self.root.to_string_lossy().into_owned(),
        }
    }

    fn session(&self, id: &str, title: Option<&str>, prompt: &str) {
        self.connection.execute(
            "INSERT INTO sessions (id, source, cwd, title, started_at) VALUES (?1, 'cli', ?2, ?3, 1700000000)",
            params![id, self.root.to_str().unwrap(), title],
        ).unwrap();
        self.connection.execute(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, 'user', ?2, 1700000001)",
            params![id, prompt],
        ).unwrap();
    }

    fn leases(&self, entries: &[(&TerminalAgentProcess, &str)]) {
        let entries: Vec<_> = entries
            .iter()
            .map(|(process, id)| json!({"pid":process.pid,"surface":"cli","session_id":id}))
            .collect();
        std::fs::write(
            self.root.join("runtime/active_sessions.json"),
            json!({"entries": entries}).to_string(),
        )
        .unwrap();
    }
}

fn metadata(processes: &[(&str, &TerminalAgentProcess)]) -> TerminalAgentList {
    TerminalAgentList {
        agents: processes
            .iter()
            .map(|(id, _)| ((*id).into(), Some("hermes".into())))
            .collect(),
        titles: processes
            .iter()
            .map(|(id, _)| ((*id).into(), "hermes".into()))
            .collect(),
        processes: processes
            .iter()
            .map(|(id, process)| ((*id).into(), (*process).clone()))
            .collect(),
    }
}

#[tokio::test]
async fn native_titles_refresh_without_completion_and_do_not_cross_same_cwd_processes() {
    let mut fixture = Fixture::new();
    let first = fixture.process();
    let second = fixture.process();
    fixture.session("first", None, "请修复\n登录接口");
    fixture.session("second", Some("另一个会话"), "Other prompt");
    fixture.leases(&[(&first, "first"), (&second, "second")]);

    let mut detected = metadata(&[("first", &first), ("second", &second)]);
    enrich(&mut detected).await;
    assert_eq!(detected.titles["first"], "请修复 登录接口");
    assert_eq!(detected.titles["second"], "另一个会话");

    // Native automatic naming or /title may finish after the reply was saved.
    fixture
        .connection
        .execute(
            "UPDATE sessions SET title = '登录接口修复' WHERE id = 'first'",
            [],
        )
        .unwrap();
    let mut detected = metadata(&[("first", &first), ("second", &second)]);
    enrich(&mut detected).await;
    assert_eq!(detected.titles["first"], "登录接口修复");
    assert_eq!(detected.titles["second"], "另一个会话");

    // /new can publish its lease before the first transcript row exists.
    fixture.leases(&[(&first, "new"), (&second, "second")]);
    let mut detected = metadata(&[("first", &first), ("second", &second)]);
    enrich(&mut detected).await;
    assert_eq!(detected.titles["first"], "hermes");
    fixture.session("new", Some("新的任务"), "New prompt");
    enrich(&mut detected).await;
    assert_eq!(detected.titles["first"], "新的任务");

    fixture.leases(&[]);
    let mut detected = metadata(&[("first", &first), ("second", &second)]);
    enrich(&mut detected).await;
    assert_eq!(detected.titles["first"], "hermes");
    assert_eq!(detected.titles["second"], "hermes");
}

#[tokio::test]
async fn native_titles_require_a_current_process_and_a_valid_unique_lease() {
    let mut fixture = Fixture::new();
    let process = fixture.process();
    fixture.session("session", Some("Native title"), "Prompt");
    fixture.leases(&[(&process, "session")]);
    let mut stale = process.clone();
    stale.start_time = "stale process".into();
    let mut elsewhere = process.clone();
    elsewhere.cwd.push_str("/another-project");
    let mut detected = metadata(&[
        ("stale", &stale),
        ("elsewhere", &elsewhere),
        ("codex", &process),
        ("missing", &process),
        ("exited", &process),
    ]);
    detected.agents.insert("codex".into(), Some("codex".into()));
    detected
        .titles
        .insert("codex".into(), "[ ! ] Action Required | Native OSC".into());
    detected.processes.remove("missing");
    detected.agents.insert("exited".into(), None);
    let original = detected.titles.clone();
    enrich(&mut detected).await;
    assert_eq!(detected.titles, original);

    fixture.session("another", Some("Wrong title"), "Prompt");
    fixture.leases(&[(&process, "session"), (&process, "another")]);
    assert!(hermes_title(process.clone()).await.is_none());
    std::fs::write(fixture.root.join("runtime/active_sessions.json"), "broken").unwrap();
    assert!(hermes_title(process.clone()).await.is_none());
    fixture.leases(&[(&process, "session")]);
    fixture
        .connection
        .execute(
            "UPDATE sessions SET ended_at = 1700000010 WHERE id = 'session'",
            [],
        )
        .unwrap();
    assert!(hermes_title(process.clone()).await.is_none());
    fixture.children[0].0.kill().unwrap();
    fixture.children[0].0.wait().unwrap();
    assert!(hermes_title(process).await.is_none());
}

#[tokio::test]
async fn terminal_metadata_returns_native_titles_only_for_the_requested_workspace() {
    use super::super::{
        TerminalManager,
        tests::{pane, tab_with},
    };
    use aow_protocol::{TerminalLayout, TerminalPaneStatus};
    use aow_terminald_client::TerminaldClient;
    use axum::{Json, Router, routing::get};

    let mut fixture = Fixture::new();
    let process = fixture.process();
    fixture.session("session", Some("Hermes 标题"), "Prompt");
    fixture.leases(&[(&process, "session")]);
    let detected = metadata(&[("one", &process), ("unrelated", &process)]);
    let app = Router::new().route(
        "/v1/agents",
        get(move || {
            let detected = detected.clone();
            async move { Json(detected) }
        }),
    );
    let socket = fixture.root.join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let manager = TerminalManager::in_memory(TerminaldClient::new(socket));
    manager.lock_state().unwrap().tabs.push(tab_with(
        TerminalLayout::Pane {
            pane_id: "one".into(),
        },
        vec![pane("one", TerminalPaneStatus::Running)],
    ));
    let detected = manager.agents(Some("/tmp")).await.unwrap();
    assert_eq!(detected.titles.len(), 1);
    assert_eq!(detected.titles["one"], "Hermes 标题");
    assert!(
        manager
            .agents(Some("/another-workspace"))
            .await
            .unwrap()
            .titles
            .is_empty()
    );
    server.abort();
}
