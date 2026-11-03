use super::*;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    routing::{get, put},
};
use instance::{Attachment, MAX_MESSAGE_BYTES, valid_message};
use std::time::Duration;

pub(super) fn router(state: WireRuntime) -> Router {
    Router::new()
        .route("/acp", get(connect))
        .route("/v1/acp/agents", get(agents))
        .route("/v1/acp/stored-sessions", get(stored_sessions))
        .route("/v1/acp/stored-sessions/{id}", get(stored_session))
        .route("/v1/acp/instances", get(list))
        .route(
            "/v1/acp/instances/{id}",
            put(create).get(describe).delete(delete),
        )
        .route("/v1/acp/instances/{id}/acp", get(attach))
        .with_state(state)
}

async fn agents(State(state): State<WireRuntime>) -> Result<Json<serde_json::Value>, ApiError> {
    let agents = state.service.agents().await.map_err(ApiError::invalid)?;
    Ok(Json(serde_json::json!({"agents": agents})))
}
async fn stored_sessions(State(state): State<WireRuntime>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"sessions": state.service.sessions(None)}))
}
async fn stored_session(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<Json<aow_acp::SessionSnapshot>, ApiError> {
    state
        .service
        .session_info(&id)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Stored ACP session not found".into()))?;
    tokio::task::spawn_blocking(move || state.service.snapshot(&id))
        .await
        .map_err(|error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .map_err(|error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn list(State(state): State<WireRuntime>) -> Json<serde_json::Value> {
    let instances: Vec<_> = state
        .instances
        .lock()
        .await
        .values()
        .map(|i| i.info())
        .collect();
    Json(serde_json::json!({"instances": instances}))
}
async fn describe(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<Json<InstanceInfo>, ApiError> {
    Ok(Json(state.get(&id).await?.info()))
}
async fn create(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
    Json(spec): Json<InstanceSpec>,
) -> Result<Response, ApiError> {
    // Creation owns its lifecycle even if an HTTP client disconnects mid-installation.
    tokio::spawn(async move {
        let (instance, created) = state.create(id, spec).await?;
        Ok((
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(instance.info()),
        )
            .into_response())
    })
    .await
    .map_err(ApiError::invalid)?
}
async fn delete(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    tokio::spawn(async move {
        state.delete(&id).await;
    })
    .await
    .map_err(ApiError::invalid)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn attach(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    upgrade(ws, state.get(&id).await?.attach()?, None)
}
async fn connect(
    State(state): State<WireRuntime>,
    Query(spec): Query<InstanceSpec>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let id = uuid::Uuid::new_v4().to_string();
    let (instance, _) = state.create(id.clone(), spec).await?;
    upgrade(ws, instance.attach()?, Some((state, id)))
}
fn upgrade(
    ws: WebSocketUpgrade,
    attachment: Attachment,
    cleanup: Option<(WireRuntime, String)>,
) -> Result<Response, ApiError> {
    let id = attachment.instance.id.clone();
    let failed_cleanup = cleanup.clone();
    let mut response = ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_failed_upgrade(move |_| {
            if let Some((state, id)) = failed_cleanup {
                tokio::spawn(async move {
                    state.delete(&id).await;
                });
            }
        })
        .on_upgrade(move |socket| async move {
            socket_loop(socket, attachment).await;
            if let Some((state, id)) = cleanup {
                state.delete(&id).await;
            }
        });
    response
        .headers_mut()
        .insert("acp-connection-id", id.parse().map_err(ApiError::invalid)?);
    Ok(response)
}
async fn socket_loop(mut socket: WebSocket, mut attachment: Attachment) {
    let mut initialized = false;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;
    let mut last_seen = tokio::time::Instant::now();
    let result: Result<()> = async {
        loop {
            tokio::select! {
                output = attachment.output.recv() => {
                    let Some(output) = output else { break; };
                    send(&mut socket, Message::Text(output.into())).await?;
                },
                input = socket.recv() => {
                    let Some(input) = input else { break; };
                    last_seen = tokio::time::Instant::now();
                    match input? {
                        Message::Text(text) => {
                            let value = match valid_message(&text) {
                                Ok(value) => value,
                                Err(code) => {
                                    send(&mut socket, Message::Text(serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":"Invalid JSON-RPC message"}}).to_string().into())).await?;
                                    continue;
                                }
                            };
                            if !initialized {
                                ensure!(value["method"] == "initialize" && value.get("id").is_some(), "First ACP message must be initialize");
                                initialized = true;
                            }
                            // Stdio is newline delimited, so normalize JSON whitespace only.
                            attachment.instance.send(serde_json::to_string(&value)?).await?;
                        },
                        Message::Close(_) => break,
                        Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {},
                    }
                },
                _ = heartbeat.tick() => {
                    ensure!(last_seen.elapsed() < Duration::from_secs(45), "ACP peer stopped responding");
                    send(&mut socket, Message::Ping(vec![].into())).await?;
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result {
        tracing::debug!(%error, "ACP WebSocket closed");
    }
    attachment.instance.stop().await;
    let _ = send(&mut socket, Message::Close(None)).await;
}
async fn send(socket: &mut WebSocket, message: Message) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await??;
    Ok(())
}
