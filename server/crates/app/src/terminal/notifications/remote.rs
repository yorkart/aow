//! Server consumes node completions and adds business labels and delivery policy.
use super::super::*;
use super::{TaskStopNotification, sources::add_sources};
use aow_runtime_client::{CompletionCursor, CompletionFrame};
use std::sync::atomic::Ordering;

impl TerminalManager {
    pub(crate) fn start_agent_notifications(
        &self,
        projects: crate::projects::ProjectService,
        notifications: crate::notifications::NotificationManager,
    ) {
        if self
            .inner
            .notifications_started
            .swap(true, Ordering::Relaxed)
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let client = self.inner.terminald.runtime().clone();
        let path = self
            .inner
            .metadata_path
            .as_ref()
            .map(|p| p.with_file_name("completion-cursor.json"));
        notifications.start();
        aow_i18n::spawn(async move {
            let mut cursor = match load_cursor(path.as_deref()) {
                Ok(cursor) => cursor,
                Err(error) => {
                    tracing::error!(%error,"cannot load completion cursor; listener stopped");
                    return;
                }
            };
            loop {
                if weak.strong_count() == 0 {
                    return;
                }
                if let Err(error) = save_cursor(path.as_deref(), cursor.as_ref()) {
                    tracing::error!(%error,"cannot persist completion cursor; listener paused");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                match client.completion_events(cursor.as_ref()).await {
                    Ok(mut socket) => {
                        let mut liveness = tokio::time::interval(Duration::from_secs(2));
                        let mut ready = false;
                        let mut received = std::time::Instant::now();
                        loop {
                            let message = tokio::select! {
                                message = socket.next() => message,
                                _ = liveness.tick() => {
                                    if weak.strong_count() == 0 { return; }
                                    if received.elapsed() > Duration::from_secs(45) { break; }
                                    continue;
                                }
                            };
                            let Some(Ok(message)) = message else { break };
                            received = std::time::Instant::now();
                            let text = match message {
                                tungstenite::Message::Text(text) => text,
                                tungstenite::Message::Ping(data) => {
                                    if socket.send(tungstenite::Message::Pong(data)).await.is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                                tungstenite::Message::Pong(_) => continue,
                                _ => break,
                            };
                            let frame = match serde_json::from_str::<CompletionFrame>(&text) {
                                Ok(frame) => frame,
                                Err(error) => {
                                    tracing::error!(%error,"invalid runtime completion frame");
                                    break;
                                }
                            };
                            match frame {
                                CompletionFrame::Ready {
                                    node_id,
                                    head,
                                    oldest,
                                    after,
                                } => {
                                    if ready
                                        || after > head
                                        || (oldest > 0 && after < oldest - 1)
                                        || cursor.as_ref().is_some_and(|c| {
                                            c.node_id != node_id || c.sequence != after
                                        })
                                    {
                                        tracing::error!(
                                            "runtime completion history changed or expired; listener stopped"
                                        );
                                        return;
                                    }
                                    if cursor.is_none() {
                                        cursor = Some(CompletionCursor {
                                            node_id,
                                            sequence: after,
                                        });
                                    }
                                    ready = true;
                                }
                                CompletionFrame::Gap { node_id, after } => {
                                    tracing::error!(%node_id,after,"runtime completion history expired; listener stopped");
                                    return;
                                }
                                CompletionFrame::Completion {
                                    node_id,
                                    sequence,
                                    event,
                                } => {
                                    let Some(current) =
                                        cursor.as_ref().filter(|c| ready && c.node_id == node_id)
                                    else {
                                        tracing::error!(
                                            "completion received without matching node readiness"
                                        );
                                        return;
                                    };
                                    if sequence <= current.sequence {
                                        continue;
                                    }
                                    if sequence != current.sequence + 1 {
                                        tracing::error!(
                                            sequence,
                                            "runtime completion sequence gap; listener stopped"
                                        );
                                        return;
                                    }
                                    let mut event = match serde_json::from_value::<
                                        TaskStopNotification,
                                    >(
                                        event
                                    ) {
                                        Ok(event) => event,
                                        Err(error) => {
                                            tracing::error!(%error,"invalid runtime completion event; listener stopped");
                                            return;
                                        }
                                    };
                                    // Execution nodes never supply business sources.
                                    event.sources.clear();
                                    let Some(inner) = weak.upgrade() else { return };
                                    let manager = TerminalManager { inner };
                                    let Ok(tabs) = manager.list_snapshot(None) else {
                                        break;
                                    };
                                    let instances: HashSet<_> = tabs
                                        .tabs
                                        .iter()
                                        .flat_map(|tab| &tab.panes)
                                        .map(|pane| pane.id.as_str())
                                        .collect();
                                    event
                                        .instance_ids
                                        .retain(|id| instances.contains(id.as_str()));
                                    if !event.instance_ids.is_empty() {
                                        let _ = manager.inner.task_completions.send(event.clone());
                                        let detected = manager.agents(None).await;
                                        add_sources(
                                            std::slice::from_mut(&mut event),
                                            &tabs.tabs,
                                            detected.as_ref().ok(),
                                            &projects,
                                        )
                                        .await;
                                        notifications.dispatch(event, &manager.inner.task_stops);
                                    }
                                    cursor = Some(CompletionCursor { node_id, sequence });
                                }
                            }
                            if let Err(error) = save_cursor(path.as_deref(), cursor.as_ref()) {
                                tracing::error!(%error,"completion cursor write failed; reconnect paused");
                                break;
                            }
                        }
                    }
                    Err(error) => tracing::warn!(%error,"runtime completion stream disconnected"),
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}
fn load_cursor(path: Option<&Path>) -> anyhow::Result<Option<CompletionCursor>> {
    let Some(path) = path else { return Ok(None) };
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn save_cursor(path: Option<&Path>, cursor: Option<&CompletionCursor>) -> anyhow::Result<()> {
    use std::io::Write;
    let (Some(path), Some(cursor)) = (path, cursor) else {
        return Ok(());
    };
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid cursor path"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".completion-cursor.{}.tmp", aow_id::new_id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec(cursor)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
