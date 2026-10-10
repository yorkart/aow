use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Additive progress view; phase names can grow without changing the connect API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub phase: String,
    pub detail: Option<String>,
    pub running: bool,
    pub elapsed_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub configured: bool,
    pub supported: bool,
    pub installed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub id: String,
    pub agent_id: String,
    pub cwd: String,
    pub agent_info: Value,
    pub capabilities: Value,
    pub auth_methods: Value,
    pub prompt_capabilities: Value,
}

/// Extensible content envelope, owned by this integration rather than the SDK.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Content {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub data: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadEntry {
    pub id: String,
    pub kind: String,
    pub content: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub kind: String,
    pub request: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: String,
    pub remote_id: String,
    pub agent_id: String,
    pub cwd: String,
    pub title: String,
    pub status: String,
    pub revision: u64,
    pub updated_at: String,
    pub entries: Vec<ThreadEntry>,
    /// Out-of-band activity must not split streaming message content.
    #[serde(default)]
    pub plan: Value,
    #[serde(default)]
    pub notices: Vec<ThreadEntry>,
    /// Display-only terminal state, accumulated independently of tool metadata patches.
    #[serde(default)]
    pub terminals: std::collections::BTreeMap<String, Value>,
    pub permissions: Vec<Permission>,
    pub modes: Value,
    pub config_options: Value,
    /// Distinguishes an advertised empty provider from legacy mode-only sessions.
    #[serde(default)]
    pub config_options_supported: bool,
    #[serde(default)]
    pub auth_required: bool,
    pub commands: Value,
    pub usage: Value,
    pub error: Option<String>,
    pub stop_reason: Option<String>,
    #[serde(skip)]
    pub(crate) pending_user_echo: String,
    #[serde(skip)]
    pub(crate) active_prompt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub remote_id: String,
    pub agent_id: String,
    pub cwd: String,
    pub title: String,
    pub status: String,
    pub updated_at: String,
}
impl From<&SessionSnapshot> for SessionInfo {
    fn from(thread: &SessionSnapshot) -> Self {
        Self {
            id: thread.id.clone(),
            remote_id: thread.remote_id.clone(),
            agent_id: thread.agent_id.clone(),
            cwd: thread.cwd.clone(),
            title: thread.title.clone(),
            status: thread.status.clone(),
            updated_at: thread.updated_at.clone(),
        }
    }
}
