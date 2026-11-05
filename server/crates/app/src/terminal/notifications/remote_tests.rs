use super::super::*;
use aow_runtime_client::{CompletionCursor, RuntimeClient, RuntimeEndpoint};
use axum::{
    Router,
    extract::{
        Query, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    routing::get,
};
use serde_json::{Value, json};

#[tokio::test]
async fn node_completion_reconnect_deduplicates_and_recovers_cursor_after_restart() {
    #[derive(Clone)]
    struct Fixture {
        requests: tokio::sync::mpsc::UnboundedSender<Value>,
        connections: Arc<std::sync::atomic::AtomicUsize>,
    }
    async fn stream(fixture: Fixture, query: Value, mut socket: WebSocket) {
        let connection = fixture
            .connections
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = fixture.requests.send(query.clone());
        let after = query["after"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        socket
            .send(Message::Text(
                json!({"type":"ready","node_id":"node-a","head":2,"oldest":1,"after":after})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let sequence = if connection == 0 { 1 } else { 2 };
        let event = |sequence| json!({"type":"completion","node_id":"node-a","sequence":sequence,"event":{"agent":"codex","session_id":"native-node-session","title":"Node title","cwd":"/remote-only/worktree","turn_id":format!("turn-{sequence}"),"conclusion":"completed on node","instance_ids":["pane"],"usage":null}});
        if connection > 0 {
            socket
                .send(Message::Text(event(1).to_string().into()))
                .await
                .unwrap();
        }
        socket
            .send(Message::Text(event(sequence).to_string().into()))
            .await
            .unwrap();
        if connection == 0 {
            let _ = socket.send(Message::Close(None)).await;
        } else {
            while socket.recv().await.is_some() {}
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    let fixture = Fixture {
        requests,
        connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let router = Router::new()
        .route(
            "/v1/agent-events",
            get(
                move |Query(query): Query<HashMap<String, String>>, ws: WebSocketUpgrade| {
                    let fixture = fixture.clone();
                    async move {
                        ws.on_upgrade(move |socket| {
                            stream(fixture, serde_json::to_value(query).unwrap(), socket)
                        })
                    }
                },
            ),
        )
        .route(
            "/v1/terminal/agents",
            get(|| async { Json(json!({"agents":{},"titles":{},"processes":{}})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = RuntimeClient::new(
        RuntimeEndpoint::new(
            &format!("http://{address}"),
            "token-0123456789abcdef0123456789abcdef".into(),
        )
        .unwrap(),
    );
    let manager = TerminalManager::persistent(root.path().into(), client.terminal()).unwrap();
    let pane:TerminalPane=serde_json::from_value(json!({"id":"pane","name":"remote","cwd":"/remote-only/worktree","shell":"/bin/sh","kind":"terminal","status":"running","rows":24,"cols":80,"created_at":"now","updated_at":"now"})).unwrap();
    manager.lock_state().unwrap().tabs.push(TerminalTab {
        id: "tab".into(),
        name: "Business tab".into(),
        name_is_custom: None,
        dock_project_id: None,
        workspace_root: "/remote-only/worktree".into(),
        layout: TerminalLayout::Pane {
            pane_id: "pane".into(),
        },
        panes: vec![pane],
        revision: 1,
        created_at: "now".into(),
        updated_at: "now".into(),
    });
    {
        let state = manager.lock_state().unwrap();
        manager.persist_locked(&state).unwrap();
    }
    let mut events = manager.subscribe_task_completions();
    let services = crate::test_support::TestServices::in_memory();
    manager.start_agent_notifications(services.projects, services.notifications);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap()
            .turn_id
            .as_deref(),
        Some("turn-1")
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(8), events.recv())
            .await
            .unwrap()
            .unwrap()
            .turn_id
            .as_deref(),
        Some("turn-2")
    );
    assert!(events.try_recv().is_err());
    assert!(received.recv().await.unwrap().get("after").is_none());
    assert_eq!(received.recv().await.unwrap()["after"], "1");
    let cursor_path = root.path().join("state/completion-cursor.json");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read(&cursor_path)
                .ok()
                .and_then(|b| serde_json::from_slice::<CompletionCursor>(&b).ok())
                .is_some_and(|c| c.sequence == 2)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(manager);
    let manager = TerminalManager::persistent(root.path().into(), client.terminal()).unwrap();
    let mut events = manager.subscribe_task_completions();
    let services = crate::test_support::TestServices::in_memory();
    manager.start_agent_notifications(services.projects, services.notifications);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap()["after"],
        "2"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), events.recv())
            .await
            .is_err()
    );
    drop(manager);
    server.abort();
}
