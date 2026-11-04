//! Node event cursor and WebSocket envelope, independent of native parsers.
use crate::{RuntimeClient, RuntimeError, RuntimeSocket};
pub use aow_runtime_contract::{CompletionCursor, CompletionFrame};
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
