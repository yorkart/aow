//! Private AoW compatibility API, available only on the local Unix socket.

use super::*;

pub(super) fn router(state: Daemon) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/rpc", post(rpc))
        .route("/v1/events", get(events))
        .route("/v1/completions/ack", post(acknowledge))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

async fn health(State(state): State<Daemon>) -> Json<Health> {
    Json(Health {
        service: "aow-acpd".into(),
        protocol: PROTOCOL_VERSION,
        instance_id: state.instance_id,
        pid: std::process::id(),
    })
}
async fn rpc(
    State(state): State<Daemon>,
    Json(call): Json<Call>,
) -> Json<std::result::Result<serde_json::Value, RpcError>> {
    if call.protocol != PROTOCOL_VERSION {
        return Json(Err(RpcError {
            code: "unsupported_protocol".into(),
            message: format!("Unsupported acpd protocol {}", call.protocol),
        }));
    }
    if *state.stopping.borrow() {
        return Json(Err(RpcError {
            code: "unavailable".into(),
            message: "ACP daemon is shutting down".into(),
        }));
    }
    // Admitted operations outlive a lost web/CLI connection. Retrying must use the original ID.
    let outcome = aow_i18n::spawn(async move {
        let mut stopped = state.stopping.clone();
        if *stopped.borrow() {
            anyhow::bail!("ACP daemon is shutting down");
        }
        tokio::select! {
            result = aow_i18n::scope(call.language, crate::dispatch::dispatch(&state.service, state.host, call.request)) => result,
            _ = stopped.changed() => anyhow::bail!("ACP daemon is shutting down"),
        }
    })
    .await;
    let result = outcome
        .map_err(anyhow::Error::from)
        .and_then(|result| result)
        .map_err(|error| RpcError {
            code: if aow_acp::is_auth_required(&error) {
                "auth_required"
            } else if matches!(
                error.to_string().as_str(),
                "ACP session not found" | "ACP connection not found"
            ) {
                "not_found"
            } else {
                "invalid"
            }
            .into(),
            message: format!("{error:#}"),
        });
    Json(result)
}
async fn events(State(state): State<Daemon>, ws: WebSocketUpgrade) -> Response {
    let changes = state.service.subscribe_sync();
    let completed = state.host.completed.subscribe();
    ws.on_upgrade(move |socket| event_socket(socket, state, changes, completed))
}
async fn send(socket: &mut WebSocket, event: &Event) -> Result<()> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.send(Message::Text(serde_json::to_string(event)?.into())),
    )
    .await??;
    Ok(())
}
async fn event_socket(
    mut socket: WebSocket,
    mut state: Daemon,
    mut changes: broadcast::Receiver<aow_acp::SyncEvent>,
    mut completed: broadcast::Receiver<Event>,
) {
    let run: Result<()> = async {
        socket.send(Message::Text("ready".into())).await?;
        let mut pending = match tokio::fs::read_dir(state.host.outbox()).await {
            Ok(pending) => Some(pending),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if let Some(pending) = &mut pending {
            while let Some(entry) = pending.next_entry().await? {
                if entry.path().extension().is_some_and(|extension| extension == "json") {
                    match tokio::fs::read(entry.path()).await {
                        Ok(bytes) => send(&mut socket, &serde_json::from_slice::<Event>(&bytes)?).await?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = state.stopping.changed() => break,
                change = changes.recv() => send(&mut socket, &Event::Sync(change?)).await?,
                completion = completed.recv() => send(&mut socket, &completion?).await?,
                _ = heartbeat.tick() => { tokio::time::timeout(std::time::Duration::from_secs(5), socket.send(Message::Ping(vec![].into()))).await??; },
                message = socket.recv() => match message {
                    Some(Ok(Message::Pong(_) | Message::Ping(_))) => {},
                    _ => break,
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = run {
        tracing::debug!(%error, "acpd subscription closed; consumer must resync");
    }
    let _ = socket.send(Message::Close(None)).await;
}
#[derive(serde::Deserialize)]
struct Ack {
    id: String,
}
async fn acknowledge(
    State(state): State<Daemon>,
    Json(ack): Json<Ack>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    uuid::Uuid::parse_str(&ack.id).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    match tokio::fs::remove_file(state.host.outbox().join(format!("{}.json", ack.id))).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    }
    Ok(Json(serde_json::json!({})))
}
