use super::*;
use axum::{
    Json,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::Duration;
#[derive(Deserialize)]
pub(super) struct Cursor {
    after: Option<u64>,
    node_id: Option<String>,
}
pub(super) async fn events(
    State(service): State<Completions>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Response {
    if cursor
        .node_id
        .as_ref()
        .is_some_and(|id| id != &service.inner.node_id)
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"message":"Completion cursor belongs to a different node"})),
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| stream(service, cursor, socket))
}
async fn send(socket: &mut WebSocket, value: aow_runtime_contract::CompletionFrame) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(serde_json::to_string(&value)?.into())),
    )
    .await??;
    Ok(())
}
async fn stream(service: Completions, cursor: Cursor, mut socket: WebSocket) {
    let mut changed = service.inner.changed.subscribe();
    let mut stopping = service.inner.stopping.clone();
    let bounds = { storage::bounds(&service.inner.log.lock().unwrap()) };
    let Ok((oldest, head)) = bounds else { return };
    let mut after = cursor.after.unwrap_or(head);
    if send(
        &mut socket,
        aow_runtime_contract::CompletionFrame::Ready {
            node_id: service.inner.node_id.clone(),
            head,
            oldest,
            after,
        },
    )
    .await
    .is_err()
    {
        return;
    }
    let mut received = std::time::Instant::now();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    loop {
        if *stopping.borrow() {
            return;
        }
        let log = service.clone();
        let batch = tokio::task::spawn_blocking(move || {
            storage::read(&log.inner.log.lock().unwrap(), after)
        })
        .await;
        match batch {
            Ok(Ok(events)) if !events.is_empty() => {
                for mut event in events {
                    let sequence = event["sequence"].as_u64().unwrap();
                    let frame = aow_runtime_contract::CompletionFrame::Completion {
                        node_id: service.inner.node_id.clone(),
                        sequence,
                        event: event["event"].take(),
                    };
                    if send(&mut socket, frame).await.is_err() {
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
                    aow_runtime_contract::CompletionFrame::Gap {
                        node_id: service.inner.node_id.clone(),
                        after,
                    },
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
