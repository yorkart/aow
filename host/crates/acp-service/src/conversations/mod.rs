//! Durable conversations belong to the execution node. HTTP serves management
//! views; the adjacent WebSocket speaks native ACP JSON-RPC.
pub(crate) mod journal;
mod native;
mod operations;
use crate::host::Host;
use aow_acp_core::AcpService;
use aow_host_contract::acp::{ConversationEvent, ServiceError, ServiceRequest};
use axum::{
    Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    routing::{get, post},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Conversations {
    service: AcpService,
    host: Arc<Host>,
    stopping: watch::Receiver<bool>,
}
impl Conversations {
    pub(crate) fn new(
        service: AcpService,
        host: Arc<Host>,
        stopping: watch::Receiver<bool>,
    ) -> Self {
        Self {
            service,
            host,
            stopping,
        }
    }
    pub(crate) fn router(&self) -> Router {
        Router::new()
            .route("/v1/acp/conversations", post(operation))
            .route("/v1/acp/conversations/events", get(events))
            .route("/v1/acp/conversations/acp", get(native::upgrade))
            .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
            .with_state(self.clone())
    }
}
async fn operation(
    State(state): State<Conversations>,
    Json(request): Json<ServiceRequest>,
) -> Json<Result<serde_json::Value, ServiceError>> {
    let language = request
        .language
        .as_deref()
        .map(aow_i18n::Language::parse)
        .unwrap_or_default();
    // Once admitted, the whole operation belongs to the node, including fences.
    // Losing the HTTP caller must not cancel cleanup between its two phases.
    let result = aow_i18n::spawn(aow_i18n::scope(language, async move {
        anyhow::ensure!(!*state.stopping.borrow(), "Runtime is stopping");
        if let Some(environment) = request.callback_environment {
            state.host.set_callback_environment(environment)?;
        }
        match &request.operation {
            aow_host_contract::acp::Operation::SetWorkspaceRemoving { cwd, removing } => {
                anyhow::ensure!(
                    std::path::Path::new(cwd).is_absolute(),
                    "Workspace path must be absolute"
                );
                if *removing {
                    state.host.filesystem.write().await.insert(cwd.into());
                }
                state.service.fence_workspace(cwd, *removing).await?;
                if !removing {
                    state
                        .host
                        .filesystem
                        .write()
                        .await
                        .remove(std::path::Path::new(cwd));
                }
                return Ok(serde_json::Value::Null);
            }
            aow_host_contract::acp::Operation::RecoverWorkspaceRemovals => {
                state.service.recover_workspace_removals().await?;
                state.host.filesystem.write().await.clear();
                return Ok(serde_json::Value::Null);
            }
            _ => {}
        }
        operations::dispatch(&state.service, request.operation, state.stopping).await
    }))
    .await
    .unwrap_or_else(|error| Err(error.into()));
    Json(result.map_err(|error| {
        ServiceError {
            code: if aow_acp_core::is_auth_required(&error) {
                "auth_required"
            } else {
                "operation_failed"
            }
            .into(),
            message: format!("{error:#}"),
        }
    }))
}
#[derive(serde::Deserialize)]
struct Cursor {
    after: Option<u64>,
    journal_id: Option<String>,
}
async fn events(
    State(state): State<Conversations>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let head = match state.host.completions.head() {
        Ok(head) => head,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if cursor
        .journal_id
        .as_ref()
        .is_some_and(|id| *id != state.host.completions.id)
        || cursor.after.is_some_and(|after| after > head)
    {
        return axum::http::StatusCode::CONFLICT.into_response();
    }
    let receiver = state.service.subscribe_sync();
    aow_i18n::http::upgrade(ws, move |socket| {
        stream(state, receiver, socket, cursor.after.unwrap_or(head), head)
    })
}
async fn stream(
    mut state: Conversations,
    mut receiver: tokio::sync::broadcast::Receiver<aow_acp_core::SyncEvent>,
    mut socket: WebSocket,
    mut after: u64,
    head: u64,
) {
    let ready = ConversationEvent::Ready {
        journal_id: state.host.completions.id.clone(),
        head,
        after,
    };
    if send(
        &mut socket,
        Message::Text(serde_json::to_string(&ready).unwrap().into()),
    )
    .await
    .is_err()
    {
        return;
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    let mut last_seen = tokio::time::Instant::now();
    let mut completion_tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = completion_tick.tick() => {
                let host = state.host.clone();
                let batch = aow_i18n::spawn_blocking(move || host.completions.after(after)).await;
                let Ok(Ok(batch)) = batch else { break; };
                for (sequence, completion) in batch {
                    let message = ConversationEvent::Completion { journal_id: state.host.completions.id.clone(), sequence, completion };
                    if send(&mut socket, Message::Text(serde_json::to_string(&message).unwrap().into())).await.is_err() { return; }
                    after = sequence;
                }
            },
            _ = state.stopping.changed() => break,
            event = receiver.recv() => {
                // A lagged consumer must reconnect and reread its subscribed heads.
                let Ok(event) = event else { break; };
                if send(&mut socket, Message::Text(serde_json::to_string(&ConversationEvent::Sync { event }).unwrap().into())).await.is_err() { break; }
            }
            input = socket.recv() => match input {
                Some(Ok(Message::Ping(data))) => {
                    last_seen = tokio::time::Instant::now();
                    if send(&mut socket, Message::Pong(data)).await.is_err() { break; }
                }
                Some(Ok(Message::Pong(_))) => { last_seen = tokio::time::Instant::now(); }
                _ => break,
            },
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > Duration::from_secs(45) || send(&mut socket, Message::Ping(vec![].into())).await.is_err() { break; }
            }
        }
    }
}
async fn send(socket: &mut WebSocket, message: Message) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await??;
    Ok(())
}
