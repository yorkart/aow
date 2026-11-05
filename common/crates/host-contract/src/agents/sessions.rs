use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeSessionQuery {
    #[serde(default)]
    pub cwd: PathBuf,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub home: Option<PathBuf>,
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, PathBuf>,
    pub reference: Option<NativeSessionReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePaneRequest {
    pub pane_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeProcessCheck {
    pub pane_id: String,
    pub process: crate::terminal::TerminalAgentProcess,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolveSessionWorktree {
    pub cwd: PathBuf,
    pub registered_roots: Vec<PathBuf>,
}
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
pub struct NativeSession {
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
pub struct NativePaneSessions {
    pub agent: String,
    pub cwd: String,
    pub title: String,
    pub process: Option<crate::terminal::TerminalAgentProcess>,
    pub live_session_id: Option<String>,
    pub sessions: Vec<NativeSession>,
    pub current: Option<NativeSessionReference>,
}
