//! Node event cursor and WebSocket envelope, independent of native parsers.
use crate::{RuntimeClient, RuntimeError, RuntimeSocket};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionCursor {
    pub node_id: String,
    pub sequence: u64,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompletionFrame {
    Ready {
        node_id: String,
        head: u64,
        oldest: u64,
        after: u64,
    },
    Completion {
        node_id: String,
        sequence: u64,
        event: serde_json::Value,
    },
    Gap {
        node_id: String,
        after: u64,
    },
}
impl RuntimeClient {
    pub async fn completion_events(
        &self,
        cursor: Option<&CompletionCursor>,
    ) -> Result<RuntimeSocket, RuntimeError> {
        let path = match cursor {
            None => "/v1/agent-events".into(),
            Some(cursor) => {
                if cursor.node_id.len() > 128
                    || cursor.node_id.is_empty()
                    || !cursor
                        .node_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(RuntimeError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Invalid node completion cursor",
                    )));
                }
                format!(
                    "/v1/agent-events?node_id={}&after={}",
                    cursor.node_id, cursor.sequence
                )
            }
        };
        self.websocket(&path).await
    }
}
