use super::*;
use std::fs::{File, OpenOptions};

fn identity(title: &str) -> Identity {
    Identity {
        agent: "codex".into(),
        process: Some(TerminalAgentProcess {
            pid: 123,
            start_time: "1000".into(),
            cwd: "/workspace/demo".into(),
        }),
        cwd: "/workspace/demo".into(),
        source_root: "/history".into(),
        target: Target::Title(title.into()),
    }
}

fn fixture(root: &Path, id: &str, modified: u64) -> AgentSessionLocator {
    let path = root.join(format!("{id}.jsonl"));
    let file = File::create(&path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(modified))
        .unwrap();
    AgentSessionLocator {
        agent: "codex",
        session_id: id.into(),
        title: "Same title".into(),
        cwd: root.into(),
        transcript_path: path,
        trusted_root: root.into(),
    }
}

fn stop(locator: &AgentSessionLocator) {
    writeln!(
        OpenOptions::new()
            .append(true)
            .open(&locator.transcript_path)
            .unwrap(),
        "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"one\",\"last_agent_message\":\"本轮完整结论\"}}}}"
    )
    .unwrap();
}

#[test]
fn refresh_keeps_existing_positions_and_new_candidates_start_at_eof() {
    let root = tempfile::tempdir().unwrap();
    let mut a = fixture(root.path(), "a", 1);
    let b = fixture(root.path(), "b", 2);
    let mut registry = Registry::default();
    registry.register("pane".into(), identity("title"), vec![a.clone()]);
    stop(&a);
    stop(&b);
    a.title = "Renamed while running".into();
    registry.register("pane".into(), identity("title"), vec![a.clone(), b.clone()]);
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "a");
    assert_eq!(events[0].title, "Renamed while running");
    stop(&b);
    assert_eq!(registry.poll()[0].session_id, "b");
    registry.register("pane".into(), identity("other"), vec![b.clone()]);
    stop(&a);
    registry.register("pane".into(), identity("title"), vec![a]);
    assert!(registry.poll().is_empty()); // re-entry has no historical cursor
}

#[test]
fn same_title_sessions_are_independent_but_shared_sessions_have_one_reader() {
    let root = tempfile::tempdir().unwrap();
    let a = fixture(root.path(), "a", 1);
    let b = fixture(root.path(), "b", 2);
    let mut registry = Registry::default();
    registry.register(
        "pane-1".into(),
        identity("title"),
        vec![a.clone(), b.clone()],
    );
    registry.register(
        "pane-2".into(),
        identity("another matching title"),
        vec![a.clone()],
    );
    assert_eq!(registry.readers.len(), 2);
    stop(&a);
    stop(&b);
    let events = registry.poll();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .find(|event| event.session_id == "a")
            .unwrap()
            .instance_ids,
        ["pane-1", "pane-2"]
    );
    registry.unregister("pane-1");
    assert_eq!(registry.readers.len(), 1);
    stop(&a);
    stop(&b);
    assert_eq!(registry.poll()[0].instance_ids, ["pane-2"]);
    registry.retain_instances(&HashSet::new());
    assert!(registry.readers.is_empty());
    assert!(registry.poll().is_empty());
}

#[test]
fn registration_is_capped_at_five_and_unchanged_binding_does_not_refill() {
    let root = tempfile::tempdir().unwrap();
    let candidates: Vec<_> = (0..7)
        .map(|i| fixture(root.path(), &i.to_string(), i))
        .collect();
    let mut registry = Registry::default();
    registry.register("pane".into(), identity("title"), candidates);
    assert_eq!(registry.readers.len(), 5);
    assert!(registry.unchanged("pane", &identity("title")));
    registry.readers.clear(); // budget eviction leaves registration fixed
    assert!(registry.unchanged("pane", &identity("title")));
    let mut restarted = identity("title");
    restarted.process.as_mut().unwrap().start_time = "2000".into();
    assert!(!registry.unchanged("pane", &restarted));
}

#[test]
fn global_budget_evicts_least_recently_modified_rollout_not_oldest_registration() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    let mut candidates = Vec::new();
    for i in 0..50 {
        let locator = fixture(root.path(), &format!("s{i}"), 100 + i);
        registry.register(
            format!("p{i}"),
            identity(&format!("title-{i}")),
            vec![locator.clone()],
        );
        candidates.push(locator);
    }
    // The oldest registration has fresh output and must survive eviction.
    stop(&candidates[0]);
    let extra = fixture(root.path(), "extra", 200);
    registry.register("extra".into(), identity("extra"), vec![extra]);
    assert_eq!(registry.readers.len(), 50);
    assert!(
        registry
            .readers
            .contains_key(&SessionKey::from(&candidates[0]))
    );
    assert!(
        !registry
            .readers
            .contains_key(&SessionKey::from(&candidates[1]))
    );
    assert!(registry.unchanged("p1", &identity("title-1")));
}

#[test]
fn new_registration_for_same_title_refreshes_all_owners() {
    let root = tempfile::tempdir().unwrap();
    let candidates: Vec<_> = (0..6)
        .map(|i| fixture(root.path(), &i.to_string(), i))
        .collect();
    let mut registry = Registry::default();
    registry.register("pane-1".into(), identity("title"), candidates[..5].to_vec());
    stop(&candidates[1]);
    stop(&candidates[5]); // before it joins the refreshed candidate set
    registry.register("pane-2".into(), identity("title"), candidates[1..].to_vec());
    assert_eq!(registry.readers.len(), 5);
    assert!(
        !registry
            .readers
            .contains_key(&SessionKey::from(&candidates[0]))
    );
    assert_eq!(
        registry.bindings["pane-1"].sessions,
        registry.bindings["pane-2"].sessions
    );
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "1");
    assert_eq!(events[0].instance_ids, ["pane-1", "pane-2"]);
}

#[tokio::test]
async fn source_labels_identify_each_owning_tab_once_and_follow_renames() {
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let locator = fixture(root.path(), "shared", 1);
    let mut registry = Registry::default();
    let mut tabs = Vec::new();
    for (tab_id, workspace, pane_ids) in [
        ("first", "/workspace/one", vec!["pane-1", "pane-2"]),
        ("second", "/workspace/two", vec!["pane-3"]),
        ("unrelated", "/workspace/three", vec!["pane-4"]),
    ] {
        let panes: Vec<_> = pane_ids
            .iter()
            .map(|id| {
                json!({
                    "id": id, "name": "Obsolete custom pane name", "name_is_custom": true, "cwd": "/somewhere/else",
                    "shell": "/bin/sh", "status": "running", "rows": 24, "cols": 80,
                    "created_at": "now", "updated_at": "now"
                })
            })
            .collect();
        let tab: TerminalTab = serde_json::from_value(json!({
            "id": tab_id, "name": tab_id, "name_is_custom": true,
            "workspace_root": workspace, "panes": panes,
            "layout": {"type": "pane", "pane_id": pane_ids[0]},
            "revision": 1, "created_at": "now", "updated_at": "now"
        }))
        .unwrap();
        tabs.push(tab);
        if tab_id != "unrelated" {
            for id in pane_ids {
                registry.register(id.into(), identity("title"), vec![locator.clone()]);
            }
        }
    }
    // Rename after registration; this must not touch the fixed session set.
    tabs[0].name = "排查性能".into();
    tabs[1].name_is_custom = Some(false);
    let detected = serde_json::from_value(json!({
        "agents": {"pane-3": "codex"}, "titles": {"pane-3": "Current visible title"}
    }))
    .unwrap();
    stop(&locator);
    let mut events = registry.poll();
    add_sources(
        &mut events,
        &tabs,
        Some(&detected),
        &crate::aow::AowManager::in_memory(),
    )
    .await;
    assert_eq!(events.len(), 1);
    let sources = &events[0].sources;
    assert_eq!(sources.len(), 2); // two panes in the first tab, one tab label
    assert_eq!(sources[0].project_name, "one");
    assert_eq!(sources[0].workspace_root, "/workspace/one");
    assert_eq!(sources[0].tab_id, "first");
    assert_eq!(sources[0].tab_name, "排查性能");
    assert_eq!(sources[1].project_name, "two");
    assert_eq!(sources[1].tab_id, "second");
    assert_eq!(sources[1].tab_name, "Current visible title");
    assert_eq!(registry.readers.len(), 1);
}

#[tokio::test]
async fn event_endpoint_requires_aow_login() {
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(crate::auth::PIN_HASH_FILE),
        format!("{:x}", md5::compute("123456")),
    )
    .unwrap();
    let mut state = AppState::new(root.path().join("frontend"));
    state.auth = crate::auth::PinAuth::persistent(root.path());
    let response = crate::build_router(state)
        .oneshot(
            axum::http::Request::get("/api/terminals/task-stops")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn event_endpoint_streams_only_new_callbacks() {
    use axum::body::HttpBody;
    use tower::ServiceExt;
    let state = AppState::new(PathBuf::from("/tmp/frontend-unused"));
    let sender = state.terminals.inner.task_stops.clone();
    let notification = TaskStopNotification {
        agent: "codex".into(),
        session_id: "session".into(),
        title: "Title".into(),
        cwd: "/workspace".into(),
        turn_id: None,
        conclusion: Some("本轮结论".into()),
        instance_ids: vec!["pane".into()],
        sources: Vec::new(),
    };
    let _ = sender.send(notification.clone()); // no replay on browser reconnect
    let response = crate::build_router(state)
        .oneshot(
            axum::http::Request::get("/api/terminals/task-stops")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    sender.send(notification).unwrap();
    let mut body = response.into_body();
    let frame = tokio::time::timeout(
        Duration::from_secs(2),
        std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let data = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(data.contains("event: task-stopped"));
    assert!(data.contains("\"session_id\":\"session\""));
}

// Exercise the actual discovery loop against the old terminald wire format,
// rather than registering readers directly. A server-only upgrade must work
// without restarting the daemon (and terminating the user's shells).
#[tokio::test]
async fn legacy_daemon_titles_register_codex_like_stops_without_process_metadata() {
    codex_like_stops_use_the_expected_environment(false).await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn native_process_config_registers_codex_like_stops_without_server_fallback() {
    codex_like_stops_use_the_expected_environment(true).await;
}

async fn codex_like_stops_use_the_expected_environment(with_process: bool) {
    use axum::body::HttpBody;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tower::ServiceExt;

    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("project");
    std::fs::create_dir(&cwd).unwrap();
    let mut environment = SessionEnvironment::new();
    let mut transcripts = Vec::new();
    for (agent, variable) in [("traecli", "TRAECLI_HOME"), ("codex", "CODEX_HOME")] {
        let home = directory.path().join(agent);
        let sessions = home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let mut locator = fixture(&sessions, agent, 1);
        locator.agent = agent;
        locator.cwd = cwd.clone();
        stop(&locator); // Existing completion must not replay when registered.
        let db = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, title TEXT,
                created_at INTEGER, updated_at INTEGER, updated_at_ms INTEGER,
                archived INTEGER, first_user_message TEXT, thread_source TEXT,
                source TEXT, agent_nickname TEXT, agent_role TEXT
            );",
        )
        .unwrap();
        db.execute(
            "INSERT INTO threads VALUES (?1, ?2, ?3, 'Same title',
                1789726500, 1789726500, 1789726500000, 0, 'Task', 'user', 'cli', '', '')",
            rusqlite::params![agent, locator.transcript_path.to_str(), cwd.to_str()],
        )
        .unwrap();
        environment.insert(variable.into(), home);
        transcripts.push(locator);
    }

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = with_process.then(|| {
        use std::process::{Command, Stdio};
        ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "terminal::sessions::tests::native_environment_fixture",
                ])
                .env("AOW_NATIVE_ENV_FIXTURE", "1")
                .envs(&environment)
                .current_dir(&cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    });
    let process = child.as_ref().map(|child| {
        let info = aow_process::info(child.0.id() as i32).unwrap();
        TerminalAgentProcess {
            pid: info.pid,
            start_time: info.start_time,
            cwd: aow_process::cwd(info.pid)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    });
    let metadata_for = |agent: &str| {
        let mut metadata = json!({
            "agents": {"pane": agent},
            "titles": {"pane": "Same title | project"}
        });
        if let Some(process) = &process {
            metadata["processes"] = json!({"pane": process});
        }
        metadata
    };
    let metadata = Arc::new(Mutex::new(metadata_for("traecli")));
    let socket = directory.path().join("terminald.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let (scanned, mut scans) = tokio::sync::mpsc::unbounded_channel();
    let daemon_metadata = metadata.clone();
    let daemon = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            assert!(line.starts_with("GET /v1/agents "));
            loop {
                line.clear();
                if stream.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let value = daemon_metadata.lock().unwrap().clone();
            let body = value.to_string();
            stream
                .get_mut()
                .write_all(format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ).as_bytes())
                .await.unwrap();
            let _ = scanned.send(value);
        }
    });

    let manager = TerminalManager::in_memory(TerminaldClient::new(socket));
    let pane: TerminalPane = serde_json::from_value(json!({
        "id": "pane", "name": "project", "cwd": cwd,
        "shell": "/bin/sh", "kind": "terminal", "status": "running",
        "rows": 24, "cols": 80, "created_at": "now", "updated_at": "now"
    }))
    .unwrap();
    manager.lock_state().unwrap().tabs.push(TerminalTab {
        id: "tab".into(),
        name: "Terminal".into(),
        name_is_custom: None,
        workspace_root: cwd.to_string_lossy().into_owned(),
        layout: TerminalLayout::Pane {
            pane_id: "pane".into(),
        },
        panes: vec![pane],
        revision: 1,
        created_at: "now".into(),
        updated_at: "now".into(),
    });
    let mut state = AppState::new(directory.path().join("frontend"));
    state.terminals = manager.clone();
    let response = crate::build_router(state)
        .oneshot(
            axum::http::Request::get("/api/terminals/task-stops")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut callbacks = manager.inner.task_stops.subscribe();
    manager.start_agent_notifications_with_environment(
        // The native path must obtain the custom stores from the child, even
        // when the server has no corresponding fallback configuration.
        if with_process {
            SessionEnvironment::new()
        } else {
            environment
        },
        Duration::from_millis(50),
        crate::aow::AowManager::in_memory(),
    );

    async fn settled(
        scans: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
        expected: &serde_json::Value,
    ) {
        // A second query proves that registration from the first has finished.
        tokio::time::timeout(Duration::from_secs(5), async {
            for _ in 0..2 {
                loop {
                    if &scans.recv().await.unwrap() == expected {
                        break;
                    }
                }
            }
        })
        .await
        .expect("notification discovery did not finish");
    }
    for locator in &transcripts {
        let expected = metadata_for(locator.agent);
        *metadata.lock().unwrap() = expected.clone();
        settled(&mut scans, &expected).await;
        assert!(
            callbacks.try_recv().is_err(),
            "replayed a historical completion"
        );
        // Rename only in the original agent store. OSC and the registered
        // title stay unchanged, so delivery must resolve the bound session ID.
        let db = rusqlite::Connection::open(
            locator
                .trusted_root
                .parent()
                .unwrap()
                .join("state_5.sqlite"),
        )
        .unwrap();
        let renamed = format!("{} renamed after registration", locator.agent);
        db.execute(
            "UPDATE threads SET title = ?1 WHERE id = ?2",
            rusqlite::params![renamed, locator.session_id],
        )
        .unwrap();
        stop(locator);
        let callback = tokio::time::timeout(Duration::from_secs(5), callbacks.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(callback.agent, locator.agent);
        assert_eq!(callback.title, renamed);
        assert_eq!(callback.instance_ids, ["pane"]);
        assert_eq!(callback.sources.len(), 1);
        assert_eq!(callback.sources[0].project_name, "project");
        assert_eq!(callback.sources[0].tab_id, "tab");
        assert_eq!(callback.sources[0].tab_name, "Terminal");
        assert_eq!(callback.conclusion.as_deref(), Some("本轮完整结论"));
        let frame = tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let event = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(event.contains("event: task-stopped"));
        assert!(event.contains(&format!("\"agent\":\"{}\"", locator.agent)));
        assert!(event.contains("\"project_name\":\"project\""));
        assert!(event.contains("\"tab_name\":\"Terminal\""));
        assert!(event.contains("\"conclusion\":\"本轮完整结论\""));
        assert!(event.contains(&format!("\"title\":\"{renamed}\"")));

        // A temporarily missing store must not suppress subsequent completions
        // or replace the latest known name with the original registration title.
        db.execute("DELETE FROM threads", []).unwrap();
        stop(locator);
        let callback = tokio::time::timeout(Duration::from_secs(5), callbacks.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(callback.title, renamed);
        assert_eq!(callback.session_id, locator.session_id);
        let frame = tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert!(
            String::from_utf8(frame.into_data().unwrap().to_vec())
                .unwrap()
                .contains(&format!("\"title\":\"{renamed}\""))
        );
        settled(&mut scans, &expected).await;
        assert!(
            callbacks.try_recv().is_err(),
            "replayed completion after rename"
        );
    }

    // Switching to Claude on a daemon without PID metadata unregisters the
    // previous title; it must not guess Claude's active session from that title.
    let expected =
        json!({"agents": {"pane": "claude"}, "titles": {"pane": "Same title | project"}});
    *metadata.lock().unwrap() = expected.clone();
    settled(&mut scans, &expected).await;
    for locator in &transcripts {
        stop(locator);
    }
    while scans.try_recv().is_ok() {}
    settled(&mut scans, &expected).await;
    assert!(
        callbacks.try_recv().is_err(),
        "kept the old agent's binding"
    );
    daemon.abort();
}
