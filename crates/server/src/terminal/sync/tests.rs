use super::*;
use crate::terminal::{TerminalError, TerminaldClient};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn snapshot(socket: &mut Socket, matches: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["event"] == "terminals"
                        && value["data"]["type"] == "snapshot"
                        && matches(&value["data"])
                    {
                        return value["data"].clone();
                    }
                }
                Message::Ping(_) => socket.flush().await.unwrap(),
                message => panic!("unexpected message: {message:?}"),
            }
        }
    })
    .await
    .unwrap()
}

async fn command(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn sockets_share_live_state_detect_exit_and_title_changes_and_resynchronize() {
    use crate::terminal::{
        CreateTerminalRequest, UpdateTerminalRequest,
        tests::{start_daemon, stop_daemon},
    };
    let directory = tempfile::tempdir().unwrap();
    let daemon_socket = directory.path().join("terminald.sock");
    let (shutdown, daemon) = start_daemon(daemon_socket.clone()).await;
    let state = crate::AppState::with_terminald_socket(Default::default(), daemon_socket.clone());
    let manager = &state.terminals;
    let tab = manager
        .create(CreateTerminalRequest {
            name: Some("initial".into()),
            cwd: Some("/tmp".into()),
            workspace_root: Some("/tmp".into()),
            shell: Some("/bin/sh".into()),
            agent_id: None,
            resume_session_id: None,
            rows: None,
            cols: None,
        })
        .await
        .unwrap();
    let pane_id = &tab.panes[0].id;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = crate::realtime::routes().with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let url = format!("ws://{address}/api/events/ws");
    let (mut first, _) = connect_async(&url).await.unwrap();
    let (mut second, _) = connect_async(&url).await.unwrap();
    command(
        &mut first,
        serde_json::json!({"action":"subscribe_terminals"}),
    )
    .await;
    let initial = snapshot(&mut first, |value| value["state"]["metadata"].is_object()).await;
    command(
        &mut second,
        serde_json::json!({"action":"subscribe_terminals"}),
    )
    .await;
    let other = snapshot(&mut second, |value| value["state"]["metadata"].is_object()).await;
    assert_eq!(initial["state"]["boot_id"], other["state"]["boot_id"]);
    assert_eq!(initial["state"]["revision"], other["state"]["revision"]);
    // Sampling an unchanged daemon does not send repeated business snapshots.
    assert!(
        tokio::time::timeout(Duration::from_millis(1800), snapshot(&mut first, |_| true))
            .await
            .is_err()
    );
    manager
        .update(
            &tab.id,
            UpdateTerminalRequest {
                name: Some("renamed".into()),
            },
        )
        .unwrap();
    snapshot(&mut first, |value| {
        value["state"]["tabs"][0]["name"] == "renamed"
    })
    .await;
    snapshot(&mut second, |value| {
        value["state"]["tabs"][0]["name"] == "renamed"
    })
    .await;
    let client = TerminaldClient::new(daemon_socket.clone());
    let mut attachment = client.attach(pane_id).await.unwrap();
    attachment
        .send(Message::Binary(bytes::Bytes::from_static(
            b"printf '\\033]2;sync-title\\007'\n",
        )))
        .await
        .unwrap();
    snapshot(&mut first, |value| {
        value["state"]["metadata"]["titles"][pane_id] == "sync-title"
    })
    .await;
    attachment
        .send(Message::Binary(bytes::Bytes::from_static(b"exit 23\n")))
        .await
        .unwrap();
    drop(attachment);
    snapshot(&mut first, |value| {
        value["state"]["tabs"][0]["panes"][0]["exit_code"] == 23
    })
    .await;
    // The subscriber observes daemon failures without deleting its last good list.
    stop_daemon(shutdown, daemon).await;
    let failed = snapshot(&mut first, |value| value["state"]["error"].is_string()).await;
    assert_eq!(failed["state"]["tabs"][0]["id"], tab.id);
    let (shutdown, daemon) = start_daemon(daemon_socket).await;
    snapshot(&mut first, |value| {
        value["state"]["error"].is_null()
            && value["state"]["tabs"][0]["panes"][0]["status"] == "running"
    })
    .await;
    command(
        &mut first,
        serde_json::json!({"action":"unsubscribe_terminals"}),
    )
    .await;
    // Ordered refresh acknowledgement includes a completed deletion.
    manager.delete_tab(&tab.id).await.unwrap();
    command(
        &mut second,
        serde_json::json!({"action":"refresh_terminals", "id":"deleted"}),
    )
    .await;
    let deleted = snapshot(&mut second, |value| value["request_id"] == "deleted").await;
    assert_eq!(deleted["state"]["tabs"], serde_json::json!([]));
    second.close(None).await.unwrap();
    let (mut reconnected, _) = connect_async(&url).await.unwrap();
    command(
        &mut reconnected,
        serde_json::json!({"action":"subscribe_terminals"}),
    )
    .await;
    let latest = snapshot(&mut reconnected, |value| value["state"]["tabs"].is_array()).await;
    assert_eq!(latest["state"]["tabs"], serde_json::json!([]));
    first.close(None).await.unwrap();
    reconnected.close(None).await.unwrap();
    server.abort();
    stop_daemon(shutdown, daemon).await;
}

#[test]
fn publishes_only_changes_and_preserves_agents_after_failed_detection() {
    let sync = TerminalSync::default();
    let mut receiver = sync.changes.subscribe();
    sync.publish_tabs(vec![]);
    assert!(receiver.has_changed().unwrap());
    let first = receiver.borrow_and_update().revision;
    sync.publish_tabs(vec![]);
    assert!(!receiver.has_changed().unwrap());
    let mut metadata = TerminalAgentList::default();
    metadata.agents.insert("pane".into(), Some("codex".into()));
    sync.publish_agents(Ok(metadata.clone()));
    assert!(receiver.borrow_and_update().revision > first);
    sync.publish_agents(Ok(metadata.clone()));
    assert!(!receiver.has_changed().unwrap());
    sync.publish_agents(Err(TerminalError::DaemonUnavailable("offline".into())));
    let snapshot = receiver.borrow_and_update().clone();
    assert_eq!(snapshot.metadata, Some(metadata));
    assert!(snapshot.agent_error.is_some());
    sync.publish_agents(Err(TerminalError::DaemonUnavailable("offline".into())));
    assert!(!receiver.has_changed().unwrap());
    sync.publish_agents(Ok(TerminalAgentList::default()));
    assert!(receiver.borrow_and_update().agent_error.is_none());
}

#[tokio::test]
async fn committed_mutations_publish_without_http_listing_and_failed_writes_do_not_publish() {
    let directory = tempfile::tempdir().unwrap();
    let manager = TerminalManager::persistent(
        directory.path().join("state"),
        TerminaldClient::new(directory.path().join("absent.sock")),
    )
    .unwrap();
    // No sampler is needed to observe committed state.
    let mut receiver = manager.inner.sync.changes.subscribe();
    let mut state = manager.lock_state().unwrap();
    manager.persist_locked(&state).unwrap();
    assert_eq!(receiver.borrow_and_update().tabs, Some(vec![]));
    let path = manager.inner.metadata_path.as_ref().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::create_dir(path).unwrap();
    state.tabs.push(crate::terminal::tests::tab_with(
        crate::terminal::TerminalLayout::Pane {
            pane_id: "pane".into(),
        },
        vec![crate::terminal::tests::pane(
            "pane",
            crate::terminal::TerminalPaneStatus::Running,
        )],
    ));
    assert!(manager.persist_locked(&state).is_err());
    assert!(!receiver.has_changed().unwrap());
}

#[tokio::test]
async fn hung_samples_timeout_independently_and_recover_without_losing_snapshots() {
    use axum::{Json, Router, extract::State, routing::get};
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Daemon {
        hang_tabs: AtomicBool,
        hang_agents: AtomicBool,
        tabs_calls: AtomicUsize,
        agents_calls: AtomicUsize,
    }

    async fn runtimes(State(daemon): State<Arc<Daemon>>) -> Json<Value> {
        daemon.tabs_calls.fetch_add(1, Ordering::SeqCst);
        if daemon.hang_tabs.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Json(serde_json::json!({"runtimes": []}))
    }

    async fn agents(State(daemon): State<Arc<Daemon>>) -> Json<TerminalAgentList> {
        daemon.agents_calls.fetch_add(1, Ordering::SeqCst);
        if daemon.hang_agents.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Json(TerminalAgentList::default())
    }

    async fn until(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sampler did not make progress");
    }

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("daemon.sock");
    let daemon = Arc::new(Daemon::default());
    let app = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(serde_json::json!({
                    "service": "terminald", "version": "test", "instance_id": "test"
                }))
            }),
        )
        .route("/v1/runtimes", get(runtimes))
        .route("/v1/agents", get(agents))
        .with_state(daemon.clone());
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let manager = TerminalManager::in_memory(TerminaldClient::new(socket));
    let receiver = manager.subscribe_snapshots();
    until(|| receiver.borrow().tabs.is_some() && receiver.borrow().metadata.is_some()).await;
    // Opening more subscribers must not create more sampler workers.
    let other = manager.subscribe_snapshots();
    daemon.hang_agents.store(true, Ordering::SeqCst);
    let agents_before = daemon.agents_calls.load(Ordering::SeqCst);
    manager.refresh_snapshots();
    until(|| daemon.agents_calls.load(Ordering::SeqCst) > agents_before).await;
    let tabs_before = daemon.tabs_calls.load(Ordering::SeqCst);
    until(|| daemon.tabs_calls.load(Ordering::SeqCst) > tabs_before).await;
    assert!(
        receiver.borrow().agent_error.is_none(),
        "list sampling must progress before the agent timeout"
    );
    assert_eq!(
        daemon.agents_calls.load(Ordering::SeqCst),
        agents_before + 1
    );
    until(|| receiver.borrow().agent_error.is_some()).await;
    assert_eq!(receiver.borrow().tabs, Some(vec![]));
    assert_eq!(
        receiver.borrow().metadata,
        Some(TerminalAgentList::default())
    );
    daemon.hang_agents.store(false, Ordering::SeqCst);
    manager.refresh_snapshots();
    until(|| receiver.borrow().agent_error.is_none()).await;

    daemon.hang_tabs.store(true, Ordering::SeqCst);
    let tabs_before = daemon.tabs_calls.load(Ordering::SeqCst);
    manager.refresh_snapshots();
    until(|| daemon.tabs_calls.load(Ordering::SeqCst) > tabs_before).await;
    let agents_before = daemon.agents_calls.load(Ordering::SeqCst);
    until(|| daemon.agents_calls.load(Ordering::SeqCst) > agents_before).await;
    assert!(
        receiver.borrow().error.is_none(),
        "agent sampling must progress before the list timeout"
    );
    assert_eq!(daemon.tabs_calls.load(Ordering::SeqCst), tabs_before + 1);
    until(|| receiver.borrow().error.is_some()).await;
    assert_eq!(receiver.borrow().tabs, Some(vec![]));
    daemon.hang_tabs.store(false, Ordering::SeqCst);
    manager.refresh_snapshots();
    until(|| receiver.borrow().error.is_none()).await;
    // Cancellation releases reconciliation's operation lock for future mutations.
    assert!(manager.inner.operation.try_lock().is_ok());
    drop(receiver);
    drop(other);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let counts = (
        daemon.tabs_calls.load(Ordering::SeqCst),
        daemon.agents_calls.load(Ordering::SeqCst),
    );
    manager.refresh_snapshots();
    tokio::time::sleep(SAMPLE_INTERVAL + Duration::from_millis(50)).await;
    assert_eq!(
        counts,
        (
            daemon.tabs_calls.load(Ordering::SeqCst),
            daemon.agents_calls.load(Ordering::SeqCst),
        )
    );
    server.abort();
}
