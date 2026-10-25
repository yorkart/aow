use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

#[tokio::test]
async fn multiplexes_snapshots_changes_and_live_notifications_and_resyncs_on_reconnect() {
    let state = AppState::new(Default::default());
    let (stops, _) = broadcast::channel(16);
    let sender = stops.clone();
    let fixture = state.clone();
    let router = Router::new().route(
        "/events",
        get(move |ws: WebSocketUpgrade| {
            let workspace = fixture.workspace_events.subscribe();
            let operations = fixture.operations.subscribe();
            let stops = sender.subscribe();
            let acp = fixture.acp.clone();
            let changes = acp.subscribe_sync();
            let terminals = fixture.terminals.clone();
            async move {
                ws.on_upgrade(move |mut socket| async move {
                    let _ = forward(
                        &mut socket,
                        workspace,
                        operations,
                        stops,
                        acp,
                        changes,
                        terminals,
                    )
                    .await;
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let url = format!("ws://{address}/events");
    let (mut socket, _) = connect_async(&url).await.unwrap();
    assert_eq!(next(&mut socket).await["event"], "workspace");
    assert_eq!(next(&mut socket).await["event"], "operations");

    state.workspace_events.terminals_changed();
    let update = next(&mut socket).await;
    assert_eq!(update["event"], "workspace");
    assert_eq!(update["data"]["terminals"], 1);
    let operation = state.operations.begin(crate::operations::Spec {
        id: "test".into(),
        kind: "test",
        source: "test",
        title: "Testing".into(),
        project_id: None,
        resource: None,
        total: Some(1),
    });
    assert_eq!(
        next(&mut socket).await["data"]["operations"][0]["id"],
        "test"
    );
    stops
        .send(crate::terminal::notifications::TaskStopNotification {
            agent: "codex".into(),
            session_id: "session".into(),
            title: "Done".into(),
            cwd: "/tmp".into(),
            turn_id: None,
            conclusion: None,
            usage: None,
            instance_ids: vec![],
            sources: vec![],
        })
        .unwrap();
    let notification = next(&mut socket).await;
    assert_eq!(notification["event"], "task-stopped");
    assert_eq!(notification["data"]["session_id"], "session");
    socket.close(None).await.unwrap();
    operation.finish(aow_operation_log::Outcome::Succeeded, "Done");
    let (mut socket, _) = connect_async(&url).await.unwrap();
    assert_eq!(next(&mut socket).await["data"]["terminals"], 1);
    assert_eq!(
        next(&mut socket).await["data"]["operations"][0]["outcome"],
        "succeeded"
    );
    // Stops are live notifications, not replayed on reconnect.
    state.workspace_events.terminals_changed();
    assert_eq!(next(&mut socket).await["event"], "workspace");
    socket.close(None).await.unwrap();
    server.abort();
}

async fn next(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                ClientMessage::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if (value["event"] == "acp" || value["event"] == "terminals")
                        && value["data"]["type"] == "hello"
                    {
                        continue;
                    }
                    return value;
                }
                ClientMessage::Ping(_) => socket.flush().await.unwrap(),
                message => panic!("unexpected message: {message:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn upgrade_requires_authentication_and_matching_origin() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let directory = tempfile::tempdir().unwrap();
    crate::auth::write_credentials(directory.path(), "admin", "test-password");
    let mut state = AppState::new(Default::default());
    state.auth = crate::auth::AuthService::persistent(directory.path());
    let router = crate::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let url = format!("ws://{address}/api/events/ws");
    assert!(
        matches!(connect_async(&url).await, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 401)
    );
    let response = reqwest::Client::new()
        .post(format!("http://{address}/api/auth/login"))
        .json(&serde_json::json!({"username": "admin", "password": "test-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert!(
        matches!(connect_async(request.clone()).await, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 403)
    );
    request
        .headers_mut()
        .insert("origin", format!("http://{address}").parse().unwrap());
    let (mut socket, _) = connect_async(request).await.unwrap();
    assert_eq!(next(&mut socket).await["event"], "workspace");
    next(&mut socket).await;
    crate::auth::write_credentials(directory.path(), "admin", "replacement-password");
    let code = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let ClientMessage::Close(frame) = socket.next().await.unwrap().unwrap() {
                break u16::from(frame.unwrap().code);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(code, 1008);
    server.abort();
}

struct AcpHost;
#[async_trait::async_trait]
impl aow_acp::AcpHost for AcpHost {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(serde_json::json!({"agent_servers":{"test":{"type":"custom","command":"/missing/adapter"}}}).to_string())
    }
    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> anyhow::Result<String> {
        unreachable!()
    }
    async fn write_text_file(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
        _: &str,
    ) -> anyhow::Result<()> {
        unreachable!()
    }
}

#[tokio::test]
async fn acp_ready_bounds_snapshot_races_reconnect_deletion_and_unsubscribe() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().canonicalize().unwrap();
    let mut state = AppState::new(Default::default());
    state.acp =
        aow_acp::AcpService::new(std::sync::Arc::new(AcpHost), Some(cwd.join("state"))).unwrap();
    let session = state
        .acp
        .prepare_session("test", cwd.clone())
        .await
        .unwrap();
    let resource = aow_acp::SyncResource::Session {
        thread_id: session.thread_id.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = routes().with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let url = format!("ws://{address}/api/events/ws");
    let command = |action: &str| {
        ClientMessage::Text(
            serde_json::json!({"action": action, "id": "thread", "resource": resource})
                .to_string()
                .into(),
        )
    };
    let (mut socket, _) = connect_async(&url).await.unwrap();
    next(&mut socket).await;
    next(&mut socket).await;
    socket.send(command("subscribe")).await.unwrap();
    let ready = next(&mut socket).await;
    assert_eq!(ready["data"]["type"], "ready");
    assert_eq!(
        ready["data"]["head"]["session"]["revision"],
        session.revision
    );
    // A commit after ready but before the initial HTTP snapshot cannot fall into a gap.
    state.acp.clear_plan(&session.thread_id).unwrap();
    let changed = next(&mut socket).await;
    let snapshot = state.acp.overview(&session.thread_id).unwrap();
    assert_eq!(
        changed["data"]["head"]["session"]["revision"],
        snapshot.revision
    );
    assert!(snapshot.revision > session.revision);
    socket
        .send(ClientMessage::Text(
            serde_json::json!({"action":"unsubscribe", "id":"thread"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket.close(None).await.unwrap();
    state.acp.clear_plan(&session.thread_id).unwrap();
    let (mut socket, _) = connect_async(&url).await.unwrap();
    next(&mut socket).await;
    next(&mut socket).await;
    socket.send(command("subscribe")).await.unwrap();
    let ready = next(&mut socket).await;
    assert!(
        ready["data"]["head"]["session"]["revision"]
            .as_u64()
            .unwrap()
            > snapshot.revision
    );
    // Read subscriptions do not create adapters or acquire control.
    assert!(
        state
            .acp
            .session_connection_info(&session.thread_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        state.acp.overview(&session.thread_id).unwrap().status,
        "disconnected"
    );
    state.acp.delete_session(&session.thread_id).await.unwrap();
    loop {
        let message = next(&mut socket).await;
        if message["data"]["head"]["deleted"] == true {
            break;
        }
    }
    socket.send(command("subscribe")).await.unwrap();
    assert_eq!(next(&mut socket).await["data"]["head"]["deleted"], true);
    socket.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn acp_coalesces_list_changes_and_keeps_urgent_session_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let service = aow_acp::AcpService::new(std::sync::Arc::new(AcpHost), None).unwrap();
    let session = service
        .prepare_session("test", directory.path().into())
        .await
        .unwrap();
    let resource = aow_acp::SyncResource::Sessions {
        cwd: session.cwd.clone(),
    };
    let mut subscriptions = acp::Subscriptions::default();
    subscriptions
        .command(
            acp::Command::Subscribe {
                id: "list".into(),
                resource: resource.clone(),
            },
            &service,
        )
        .unwrap();
    let mut change = service.sync_event(&resource);
    for version in 1..100 {
        change.version = version;
        assert!(subscriptions.change(change.clone()).is_none());
    }
    assert!(subscriptions.due().is_empty());
    tokio::time::sleep(Duration::from_millis(510)).await;
    let messages = subscriptions.due();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["head"]["version"], 99);
    let resource = aow_acp::SyncResource::Session {
        thread_id: session.thread_id,
    };
    subscriptions
        .command(
            acp::Command::Subscribe {
                id: "session".into(),
                resource: resource.clone(),
            },
            &service,
        )
        .unwrap();
    let mut change = service.sync_event(&resource);
    change.urgent = false;
    assert!(subscriptions.change(change.clone()).is_none());
    change.urgent = true;
    change.version += 1;
    assert!(subscriptions.change(change).is_some());
    subscriptions
        .command(
            acp::Command::Unsubscribe {
                id: "session".into(),
            },
            &service,
        )
        .unwrap();
    assert!(
        subscriptions
            .change(service.sync_event(&resource))
            .is_none()
    );
}
