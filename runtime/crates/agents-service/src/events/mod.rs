use anyhow::Result;
use aow_agents_core::completions::Completions;
use axum::{
    Json,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum::{Router, routing::get};
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::Duration;
#[derive(Deserialize)]
struct Cursor {
    after: Option<u64>,
    node_id: Option<String>,
}
async fn events(
    State(service): State<Completions>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Response {
    if cursor
        .node_id
        .as_ref()
        .is_some_and(|id| id != service.node_id())
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"message":"Completion cursor belongs to a different node"})),
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| stream(service, cursor, socket))
}
async fn send(socket: &mut WebSocket, value: serde_json::Value) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await??;
    Ok(())
}
async fn stream(service: Completions, cursor: Cursor, mut socket: WebSocket) {
    let mut changed = service.subscribe();
    let mut stopping = service.stopping();
    let bounds = { service.bounds() };
    let Ok((oldest, head)) = bounds else { return };
    let mut after = cursor.after.unwrap_or(head);
    if send(&mut socket,serde_json::json!({"type":"ready","node_id":service.node_id(),"head":head,"oldest":oldest,"after":after})).await.is_err(){return;}
    let mut received = std::time::Instant::now();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    loop {
        if *stopping.borrow() {
            return;
        }
        let log = service.clone();
        let batch = tokio::task::spawn_blocking(move || log.read(after)).await;
        match batch {
            Ok(Ok(events)) if !events.is_empty() => {
                for mut event in events {
                    event["node_id"] = serde_json::json!(service.node_id());
                    let sequence = event["sequence"].as_u64().unwrap();
                    if send(&mut socket, event).await.is_err() {
                        return;
                    }
                    after = sequence;
                }
                continue;
            }
            Ok(Ok(_)) => {}
            _ => {
                let _ = send(
                    &mut socket,
                    serde_json::json!({"type":"gap","node_id":service.node_id(),"after":after}),
                )
                .await;
                return;
            }
        }
        tokio::select! {
            _ = stopping.changed() => return,
            result = changed.changed() => if result.is_err() { return; },
            message = socket.next() => match message {
                Some(Ok(Message::Ping(data))) => {
                    received = std::time::Instant::now();
                    if !matches!(tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Pong(data))).await, Ok(Ok(()))) { return; }
                }
                Some(Ok(Message::Pong(_))) => { received = std::time::Instant::now(); }
                _ => return,
            },
            _ = heartbeat.tick() => {
                if received.elapsed() > Duration::from_secs(45) { return; }
                if !matches!(tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { return; }
            }
        }
    }
}

pub fn router(service: Completions) -> Router {
    Router::new()
        .route("/v1/agent-events", get(events))
        .with_state(service)
}

#[cfg(test)]
mod tests;
