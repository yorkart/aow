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
                "terminal::sessions::tests::native_environment_fixture",
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

#[tokio::test]
async fn metadata_preserves_osc_titles_and_filters_other_workspaces() {
    use super::super::{
        TerminalManager,
        tests::{pane, tab_with},
    };
    use aow_protocol::{TerminalLayout, TerminalPaneStatus};
    use aow_terminald_client::TerminaldClient;
    use axum::{Json, Router, routing::get};

    let directory = tempfile::tempdir().unwrap();
    let agent = aow_agents::KNOWN_AGENTS
        .iter()
        .find(|agent| agent.session_titles().is_none())
        .unwrap();
    let detected = TerminalAgentList {
        agents: [
            ("one".into(), Some(agent.id().into())),
            ("unrelated".into(), None),
        ]
        .into(),
        titles: [
            ("one".into(), "[ ! ] Action Required | Native OSC".into()),
            ("unrelated".into(), "Other workspace".into()),
        ]
        .into(),
        ..Default::default()
    };
    let app = Router::new().route(
        "/v1/agents",
        get(move || {
            let detected = detected.clone();
            async move { Json(detected) }
        }),
    );
    let socket = directory.path().join("daemon.sock");
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
    assert_eq!(detected.titles["one"], "[ ! ] Action Required | Native OSC");
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

#[tokio::test]
#[ignore = "requires AOW_TITLE_TEST_PID, AOW_TITLE_TEST_SESSION_ID and AOW_TITLE_TEST_EXPECTED; reads a live process without terminal input"]
async fn native_process_title_matches_the_live_session() {
    use super::super::{
        TerminalManager,
        tests::{pane, tab_with},
    };
    use aow_agents::{
        process::{ProcessInfo, recognize_process},
        sessions::tracking::{AgentSessionTracker, SessionResolution, SessionTarget},
    };
    use aow_protocol::{TerminalLayout, TerminalPaneStatus};
    use aow_terminald_client::TerminaldClient;
    use axum::{Json, Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;
    let pid = std::env::var("AOW_TITLE_TEST_PID")
        .unwrap()
        .parse()
        .unwrap();
    let expected = std::env::var("AOW_TITLE_TEST_EXPECTED").unwrap();
    let session_id = std::env::var("AOW_TITLE_TEST_SESSION_ID").unwrap();
    let command = aow_process::command(pid);
    let arguments: Vec<_> = command
        .arguments
        .split(|b| *b == 0)
        .map(|arg| std::str::from_utf8(arg).unwrap_or(""))
        .collect();
    let agent = recognize_process(&ProcessInfo::new(
        command
            .executable
            .as_deref()
            .and_then(std::path::Path::to_str),
        &arguments,
    ))
    .expect("live agent must be recognized");
    let process = TerminalAgentProcess {
        pid,
        start_time: aow_process::info(pid).unwrap().start_time,
        cwd: aow_process::cwd(pid)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    };
    let environment = sessions::process_environment(&process).unwrap();
    assert_eq!(
        agent
            .session_tracking()
            .unwrap()
            .resolve_live_session(LiveSessionContext {
                pid: Some(pid),
                cwd: &process.cwd,
                title: "",
                environment: &environment,
            })
            .await,
        SessionResolution::Resolved(SessionTarget::Id(session_id.clone()))
    );
    let detected = TerminalAgentList {
        agents: [("live".into(), Some(agent.id().into()))].into(),
        processes: [("live".into(), process)].into(),
        ..Default::default()
    };
    // Exercise the production HTTP metadata handler against this real process.
    // Only the daemon transport is isolated, so the installed daemon need not
    // be restarted (which would terminate the user's running terminals).
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("daemon.sock");
    let daemon = Router::new().route(
        "/v1/agents",
        get(move || {
            let detected = detected.clone();
            async move { Json(detected) }
        }),
    );
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, daemon).await.unwrap() });
    let manager = TerminalManager::in_memory(TerminaldClient::new(socket.clone()));
    manager.lock_state().unwrap().tabs.push(tab_with(
        TerminalLayout::Pane {
            pane_id: "live".into(),
        },
        vec![pane("live", TerminalPaneStatus::Running)],
    ));
    let mut state = crate::AppState::with_terminald_socket(directory.path().into(), socket);
    state.terminals = manager;
    let response = crate::build_router(state)
        .oneshot(
            Request::builder()
                .uri("/api/terminals/agents?workspace_root=%2Ftmp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let detected: TerminalAgentList = serde_json::from_slice(&bytes).unwrap();
    server.abort();
    assert_eq!(detected.titles["live"], expected);
    eprintln!(
        "Verified live PID {pid}, agent {}, session {session_id}, title {:?}",
        agent.id(),
        detected.titles["live"]
    );
    if let Some(path) = std::env::var_os("AOW_TITLE_TEST_EXPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&detected).unwrap()).unwrap();
    }
}
