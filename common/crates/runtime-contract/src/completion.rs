use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionCursor {
    pub node_id: String,
    pub sequence: u64,
}
#[derive(Debug, Serialize, Deserialize)]
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
