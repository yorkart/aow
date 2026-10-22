//! Multiplex live updates on a WebSocket so tabs cannot exhaust HTTP/1.1 slots.
mod acp;
mod terminals;

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
    let acp_events = state.acp.subscribe_sync();
    Ok(crate::locale::upgrade(
        websocket.max_message_size(8192),
        move |mut socket| async move {
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
                        reason: aow_i18n::text("validation.authentication_expired").into(),
                    })))).await;
                }
                _ = forward(&mut socket, workspace, operations, stops, state.acp, acp_events, state.terminals) => {}
            }
        },
    ))
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
    acp_service: aow_acp::AcpService,
    mut acp_events: broadcast::Receiver<aow_acp::SyncEvent>,
    terminal_manager: crate::terminal::TerminalManager,
) -> Result<(), ()> {
    // Never hold a watch borrow across a send/await; publishers need its write lock.
    let initial = event("workspace", workspace.borrow_and_update().clone());
    send(socket, initial).await?;
    let mut subscriptions = acp::Subscriptions::default();
    let mut flush = tokio::time::interval(Duration::from_millis(50));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let initial = event("operations", operations.borrow_and_update().clone());
    send(socket, initial).await?;
    send(
        socket,
        event("acp", serde_json::json!({"type": "hello", "protocol": 1})),
    )
    .await?;
    send(
        socket,
        event(
            "terminals",
            serde_json::json!({"type": "hello", "protocol": 1}),
        ),
    )
    .await?;
    let mut terminal_subscription = terminals::Subscription::new(terminal_manager);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_pong = tokio::time::Instant::now();
    loop {
        let message = tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Pong(_))) => last_pong = tokio::time::Instant::now(),
                    Some(Ok(Message::Ping(_))) => {}, // Axum replies automatically.
                    Some(Ok(Message::Text(text))) => {
                        let command: Command = serde_json::from_str(&text).map_err(|_| ())?;
                        match command {
                            Command::Terminals(command) => {
                                if let Some(snapshot) = terminal_subscription.command(command)? {
                                    send(socket, event("terminals", snapshot)).await?;
                                }
                            }
                            Command::Acp(command) => {
                                if let Some(ready) = subscriptions.command(command, &acp_service)? {
                                    send(socket, event("acp", ready)).await?;
                                }
                            }
                        }
                    }
                    _ => return Ok(()),
                }
                continue;
            }
            result = workspace.changed() => {
                result.map_err(|_| ())?;
                event("workspace", workspace.borrow_and_update().clone())
            }
            snapshot = terminal_subscription.changed() => {
                event("terminals", snapshot?)
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
            result = acp_events.recv() => {
                match result {
                    Ok(change) => match subscriptions.change(change) {
                        Some(change) => event("acp", change),
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        send(socket, event("acp", serde_json::json!({"type": "resync"}))).await?;
                        return Ok(());
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
            _ = flush.tick() => {
                for change in subscriptions.due() { send(socket, event("acp", change)).await?; }
                continue;
            }
            _ = heartbeat.tick() => {
                if last_pong.elapsed() > Duration::from_secs(45) { return Ok(()); }
                Message::Ping(Vec::new().into())
            }
        };
        send(socket, message).await?;
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Command {
    Terminals(terminals::Command),
    Acp(acp::Command),
}

#[cfg(test)]
mod tests;
