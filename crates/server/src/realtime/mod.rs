//! Multiplex live updates on a WebSocket so tabs cannot exhaust HTTP/1.1 slots.

use std::time::Duration;

use axum::{
    Extension, Router,
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
    routing::get,
};
use serde::Serialize;
use tokio::sync::{broadcast, watch};

use crate::{AppState, HttpError, auth::SessionAccess};

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/api/events/ws", get(upgrade))
}

async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    access: Option<Extension<SessionAccess>>,
    websocket: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    crate::auth::check_origin(&headers)?;
    // Subscribe before upgrading so updates during the handshake are retained.
    let workspace = state.workspace_events.subscribe();
    let operations = state.operations.subscribe();
    let stops = state.terminals.subscribe_task_stops();
    Ok(websocket.max_message_size(1024).on_upgrade(move |mut socket| async move {
        let revoked = async {
            match access {
                Some(access) => access.revoked().await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            biased;
            _ = revoked => {
                let _ = tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Close(Some(CloseFrame {
                    code: 1008,
                    reason: "Authentication expired".into(),
                })))).await;
            }
            _ = forward(&mut socket, workspace, operations, stops) => {}
        }
    }))
}

fn event(name: &str, data: impl Serialize) -> Message {
    Message::Text(
        serde_json::json!({ "event": name, "data": data })
            .to_string()
            .into(),
    )
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn forward(
    socket: &mut WebSocket,
    mut workspace: watch::Receiver<crate::workspace_events::Snapshot>,
    mut operations: watch::Receiver<crate::operations::Snapshot>,
    mut stops: broadcast::Receiver<crate::terminal::notifications::TaskStopNotification>,
) -> Result<(), ()> {
    // Never hold a watch borrow across a send/await; publishers need its write lock.
    let initial = event("workspace", workspace.borrow_and_update().clone());
    send(socket, initial).await?;
    let initial = event("operations", operations.borrow_and_update().clone());
    send(socket, initial).await?;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_pong = tokio::time::Instant::now();
    loop {
        let message = tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Pong(_))) => last_pong = tokio::time::Instant::now(),
                    Some(Ok(Message::Ping(_))) => {}, // Axum replies automatically.
                    _ => return Ok(()), // This stream accepts no application commands.
                }
                continue;
            }
            result = workspace.changed() => {
                result.map_err(|_| ())?;
                event("workspace", workspace.borrow_and_update().clone())
            }
            result = operations.changed() => {
                result.map_err(|_| ())?;
                event("operations", operations.borrow_and_update().clone())
            }
            result = stops.recv() => {
                match result {
                    Ok(stop) => event("task-stopped", stop),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
            _ = heartbeat.tick() => {
                if last_pong.elapsed() > Duration::from_secs(45) { return Ok(()); }
                Message::Ping(Vec::new().into())
            }
        };
        send(socket, message).await?;
    }
}

#[cfg(test)]
mod tests;
