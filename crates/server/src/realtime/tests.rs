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
            async move {
                ws.on_upgrade(move |mut socket| async move {
                    let _ = forward(&mut socket, workspace, operations, stops).await;
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
                ClientMessage::Text(text) => return serde_json::from_str(&text).unwrap(),
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
