use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionReference {
    pub node_id: String,
    pub id: String,
    #[serde(default)]
    pub native_session: Option<crate::NativeSessionReference>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub sequence: u64,
    pub event: Value,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub id: String,
    pub node_id: String,
    pub head: u64,
    pub terminal: bool,
    pub events: Vec<ExecutionEvent>,
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_session: Option<crate::NativeSessionRecord>,
}
