use serde::Deserialize;
use serde_json::Value;
use tokio::sync::watch;

use crate::terminal::{TerminalManager, sync::Snapshot};

#[derive(Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub(super) enum Command {
    #[serde(rename = "subscribe_terminals")]
    Subscribe,
    #[serde(rename = "unsubscribe_terminals")]
    Unsubscribe,
    #[serde(rename = "refresh_terminals")]
    Refresh { id: String },
}

pub(super) struct Subscription {
    manager: TerminalManager,
    receiver: Option<watch::Receiver<Snapshot>>,
}

impl Subscription {
    pub(super) fn new(manager: TerminalManager) -> Self {
        Self {
            manager,
            receiver: None,
        }
    }

    pub(super) fn command(&mut self, command: Command) -> Result<Option<Value>, ()> {
        match command {
            Command::Subscribe => {
                let receiver = self
                    .receiver
                    .get_or_insert_with(|| self.manager.subscribe_snapshots());
                let snapshot = receiver.borrow_and_update().clone();
                Ok(Some(message(snapshot, None)))
            }
            Command::Unsubscribe => {
                self.receiver = None;
                Ok(None)
            }
            Command::Refresh { id } => {
                if id.len() > 128 {
                    return Err(());
                }
                let Some(receiver) = &mut self.receiver else {
                    return Ok(None);
                };
                self.manager.refresh_snapshots();
                // Mutations publish before their HTTP response. This acknowledgement
                // therefore bounds any older snapshots already queued on the socket.
                let snapshot = receiver.borrow_and_update().clone();
                Ok(Some(message(snapshot, Some(id))))
            }
        }
    }

    pub(super) async fn changed(&mut self) -> Result<Value, ()> {
        let Some(receiver) = &mut self.receiver else {
            return std::future::pending().await;
        };
        receiver.changed().await.map_err(|_| ())?;
        let snapshot = receiver.borrow_and_update().clone();
        Ok(message(snapshot, None))
    }
}

fn message(snapshot: Snapshot, request_id: Option<String>) -> Value {
    serde_json::json!({"type": "snapshot", "state": snapshot, "request_id": request_id})
}
