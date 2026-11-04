use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSessionReference {
    pub node_id: String,
    pub handle: String,
    pub agent: String,
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeSessionRecord {
    pub id: String,
    pub agent: String,
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,
    pub reference: NativeSessionReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePaneSessionRecord {
    pub agent: String,
    pub cwd: String,
    pub title: String,
    pub process: Option<aow_protocol::TerminalAgentProcess>,
    pub live_session_id: Option<String>,
    pub sessions: Vec<NativeSessionRecord>,
    pub current: Option<NativeSessionReference>,
}
