use super::*;
use crate::{HostClient, HostSocket};
use anyhow::{Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;
impl AcpClient {
    pub(super) async fn ensure_events(&self, host: &HostClient) -> Result<()> {
        let mut task = self.0.event_task.lock().await;
        if task.as_ref().is_some_and(|task| !task.is_finished()) {
            return Ok(());
        }
        let cursor = self
            .0
            .observer
            .as_ref()
            .map(|observer| observer.completion_cursor())
            .transpose()?
            .flatten();
        let (socket, mut cursor) = connect(host, cursor.as_ref()).await?;
        if let Some(observer) = &self.0.observer {
            observer.save_completion_cursor(&cursor)?;
        }
        let inner = Arc::downgrade(&self.0);
        let host = host.clone();
        *task = Some(tokio::spawn(async move {
            let mut socket = socket;
            loop {
                while let Some(message) = socket.next().await {
                    let Some(inner) = inner.upgrade() else {
                        return;
                    };
                    match message {
                        Ok(Message::Text(text)) => {
                            let Ok(frame) = serde_json::from_str::<ConversationEvent>(&text) else {
                                break;
                            };
                            match frame {
                                ConversationEvent::Completion {
                                    journal_id,
                                    sequence,
                                    completion,
                                } => {
                                    if journal_id != cursor.journal_id
                                        || sequence != cursor.sequence + 1
                                    {
                                        break;
                                    }
                                    if let Some(observer) = &inner.observer {
                                        observer.turn_completed(completion).await;
                                    }
                                    let acknowledged = ConversationCursor {
                                        journal_id,
                                        sequence,
                                    };
                                    if let Some(observer) = &inner.observer
                                        && let Err(error) =
                                            observer.save_completion_cursor(&acknowledged)
                                    {
                                        tracing::error!(%error, "Unable to acknowledge host completion");
                                        return;
                                    }
                                    cursor = acknowledged;
                                }
                                ConversationEvent::Sync { event } => {
                                    if let Some(observer) = &inner.observer {
                                        observer.session_metadata_changed();
                                    }
                                    let _ = inner.events.send(event);
                                }
                                ConversationEvent::Ready { .. } => break,
                            }
                        }
                        Ok(Message::Ping(data)) => {
                            if socket.send(Message::Pong(data)).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Pong(_)) => {}
                        _ => break,
                    }
                }
                loop {
                    if inner.strong_count() == 0 {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if let Ok((connected, resumed)) = connect(&host, Some(&cursor)).await {
                        socket = connected;
                        cursor = resumed;
                        break;
                    }
                }
                // Subscribed views must reread heads changed during the transport gap.
                let Some(current) = inner.upgrade() else {
                    return;
                };
                let resources: Vec<_> = current.resources.lock().unwrap().iter().cloned().collect();
                for resource in resources {
                    let response: Result<serde_json::Value, ServiceError> = match host
                        .post_json(
                            "/v1/acp/conversations",
                            &ServiceRequest {
                                operation: Operation::SyncEvent { resource },
                                language: None,
                                callback_environment: None,
                            },
                        )
                        .await
                    {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    if let Ok(value) = response
                        && let Ok(event) = serde_json::from_value::<SyncEvent>(value)
                    {
                        let _ = current.events.send(event);
                    }
                }
                if let Some(observer) = &current.observer {
                    observer.session_metadata_changed();
                }
            }
        }));
        Ok(())
    }
}
async fn connect(
    host: &HostClient,
    cursor: Option<&ConversationCursor>,
) -> Result<(HostSocket, ConversationCursor)> {
    let path = match cursor {
        None => "/v1/acp/conversations/events".into(),
        Some(cursor) => {
            ensure!(
                !cursor.journal_id.is_empty()
                    && cursor
                        .journal_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "Invalid conversation event cursor"
            );
            format!(
                "/v1/acp/conversations/events?after={}&journal_id={}",
                cursor.sequence, cursor.journal_id
            )
        }
    };
    let mut socket = host.websocket(&path).await?;
    let ready = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Missing event subscription acknowledgement"))??;
    let Message::Text(text) = ready else {
        bail!("Invalid event subscription acknowledgement");
    };
    let ConversationEvent::Ready {
        journal_id,
        head,
        after,
    } = serde_json::from_str(&text)?
    else {
        bail!("Invalid event subscription acknowledgement");
    };
    ensure!(
        after <= head
            && cursor
                .is_none_or(|cursor| cursor.journal_id == journal_id && cursor.sequence == after),
        "Host conversation event history changed"
    );
    Ok((
        socket,
        ConversationCursor {
            journal_id,
            sequence: after,
        },
    ))
}
