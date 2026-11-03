use crate::AppState;
use aow_acpd_client::Event;
use std::time::Duration;

impl AppState {
    /// Reconnect independently of browser subscriptions; completion outbox is replayed until ACK.
    pub fn start_acp_notifications(&self) {
        if !self.acp.is_remote() {
            return;
        }
        let state = self.clone();
        aow_i18n::spawn(async move {
            let mut connected_once = false;
            let mut recent = std::collections::VecDeque::new();
            loop {
                if let Ok(mut events) = state.acp.subscribe_events().await {
                    if !connected_once {
                        if state.acp.recover_workspace_removals().await.is_err() {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                        connected_once = true;
                    }
                    state.workspace_events.instances_changed();
                    while let Ok(event) = events.recv().await {
                        match event {
                            Event::Sync(event) => {
                                if event.urgent {
                                    state.workspace_events.instances_changed();
                                }
                            }
                            Event::Completed { id, completion } => {
                                if !recent.contains(&id) {
                                    super::dispatch(
                                        &state.aow,
                                        &state.terminals.task_stop_sender(),
                                        completion,
                                    )
                                    .await;
                                    recent.push_back(id.clone());
                                    if recent.len() > 1024 {
                                        recent.pop_front();
                                    }
                                }
                                if let Err(error) = state.acp.acknowledge_completion(&id).await {
                                    tracing::warn!(%error, "ACP completion ACK failed; will replay on reconnect");
                                    break;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
}
