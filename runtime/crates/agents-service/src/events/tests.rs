use aow_agents_core::completions::{Completion, Completions};
use std::time::Duration;
fn event() -> Completion {
    Completion {
        agent_display_name: "Codex".into(),
        agent: "codex".into(),
        session_id: "native".into(),
        title: "Node title".into(),
        cwd: "/remote-only/worktree".into(),
        turn_id: Some("turn".into()),
        conclusion: Some("complete".into()),
        usage: None,
        instance_ids: vec!["pane".into()],
    }
}
#[tokio::test]
async fn websocket_replays_persisted_events_and_rejects_another_node() {
    use axum::Router;
    use futures_util::StreamExt;
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    let root = tempfile::tempdir().unwrap();
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let service = Completions::open(root.path(), "node-a".into(), stopping).unwrap();
    service.append(&[event(), event()]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router: Router = super::router(service.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let base = format!("ws://{address}/v1/agent-events");
    async fn next(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                    return serde_json::from_str(&text).unwrap();
                }
            }
        })
        .await
        .unwrap()
    }
    let (mut socket, _) = connect_async(format!("{base}?node_id=node-a&after=1"))
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await["type"], "ready");
    assert_eq!(next(&mut socket).await["sequence"], 2);
    socket.close(None).await.unwrap();
    let (mut socket, _) = connect_async(format!("{base}?node_id=node-a&after=2"))
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await["after"], 2);
    service.append(&[event()]).unwrap();
    assert_eq!(next(&mut socket).await["sequence"], 3);
    assert!(
        connect_async(format!("{base}?node_id=node-b&after=2"))
            .await
            .is_err()
    );
    let (mut expired, _) = connect_async(format!("{base}?after=100")).await.unwrap();
    assert_eq!(next(&mut expired).await["type"], "ready");
    assert_eq!(next(&mut expired).await["type"], "gap");
    stop.send_replace(true);
    server.abort();
}
