use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Additive progress view; phase names can grow without changing the connect API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub operation_id: String,
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
    /// Protocol message identity, separate from the local entry/rendering ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub content: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub kind: String,
    pub request: Value,
}

/// Locally observed prompt lifecycle, keyed by the first user content entry.
/// Imported history without this record has an unknown outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionTurn {
    pub user_entry_id: String,
    pub status: String,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}

/// Connection recovery is independent of recorded prompt outcomes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecovery {
    pub attempt_id: String,
    pub phase: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub thread_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub agent_id: String,
    pub cwd: String,
    pub title: String,
    pub status: String,
    /// Imported metadata is hydrated only when the user opens this conversation.
    #[serde(default)]
    pub needs_load: bool,
    #[serde(default)]
    pub recovery: Option<SessionRecovery>,
    pub revision: u64,
    /// Distinguishes live incarnations across server restarts and history reloads.
    #[serde(default)]
    pub sync_generation: String,
    pub updated_at: String,
    pub entries: Vec<ThreadEntry>,
    #[serde(default)]
    pub turns: Vec<SessionTurn>,
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
    /// Live response baseline; restored sessions may have progressed elsewhere.
    #[serde(skip)]
    pub(crate) last_prompt_usage: Option<Value>,
    pub error: Option<String>,
    pub stop_reason: Option<String>,
    #[serde(skip)]
    pub(crate) pending_user_echo: String,
    #[serde(skip)]
    pub(crate) pending_user_images: Vec<Value>,
    #[serde(skip)]
    pub(crate) active_prompt: Option<String>,
    #[serde(skip)]
    pub(crate) changes: crate::acp_thread::updates::ChangeLog,
    /// Browser projection only; live/persisted snapshots retain the canonical transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<Vec<TurnOverview>>,
    #[serde(skip)]
    pub(crate) overview_cache: crate::acp_thread::overview::OverviewCache,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnOverview {
    pub id: String,
    pub revision: u64,
    pub user: Vec<ThreadEntry>,
    pub conclusion: Vec<ThreadEntry>,
    pub tool_count: usize,
    pub progress_count: usize,
    pub running: bool,
    pub waiting: bool,
    pub compacting: bool,
    pub status: Option<String>,
    pub error: Option<String>,
    pub permission_tool_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TurnOverviewPatch {
    pub id: String,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<Vec<ThreadEntry>>,
    pub conclusion: Vec<ThreadEntry>,
    pub tool_count: usize,
    pub progress_count: usize,
    pub running: bool,
    pub waiting: bool,
    pub compacting: bool,
    pub status: Option<String>,
    pub error: Option<String>,
    pub permission_tool_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionUpdates {
    Snapshot {
        snapshot: Box<SessionSnapshot>,
    },
    Unchanged,
    Overview {
        from_revision: u64,
        snapshot: Box<SessionSnapshot>,
        turns: Vec<TurnOverviewPatch>,
    },
    Delta {
        thread_id: String,
        sync_generation: String,
        from_revision: u64,
        revision: u64,
        updated_at: String,
        entries: Vec<EntryPatch>,
        terminals: Vec<TerminalPatch>,
        fields: serde_json::Map<String, Value>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryPatch {
    Replace {
        index: usize,
        entry: ThreadEntry,
    },
    AppendText {
        index: usize,
        id: String,
        text: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalPatch {
    Replace { id: String, terminal: Value },
    AppendOutput { id: String, text: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub thread_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub agent_id: String,
    pub cwd: String,
    pub title: String,
    pub status: String,
    pub updated_at: String,
}

/// A newly completed prompt, never emitted by history loading or snapshot reads.
#[derive(Clone, Debug)]
pub struct TurnCompletion {
    pub session: SessionInfo,
    pub turn_id: String,
    pub conclusion: Option<String>,
    /// Per-turn delta of consecutive response counters, when both are known.
    pub usage: Option<Value>,
}
impl From<&SessionSnapshot> for SessionInfo {
    fn from(thread: &SessionSnapshot) -> Self {
        Self {
            thread_id: thread.thread_id.clone(),
            session_id: thread.session_id.clone(),
            agent_id: thread.agent_id.clone(),
            cwd: thread.cwd.clone(),
            title: thread.title.clone(),
            status: thread.status.clone(),
            updated_at: thread.updated_at.clone(),
        }
    }
}

/// Metadata selected explicitly in the external-agent import dialog.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionImport {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}
