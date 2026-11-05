use super::SessionInfo;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SyncResource {
    Session {
        thread_id: String,
    },
    Sessions {
        cwd: String,
    },
    Connection {
        thread_id: String,
    },
    Requests {
        connection_id: String,
    },
    Operation {
        agent_id: String,
        cwd: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionVersion {
    pub generation: String,
    pub revision: u64,
    pub info: SessionInfo,
}

/// All resources use invalidations followed by HTTP reads. Session metadata is
/// included so hidden views can track status without downloading their bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncEvent {
    pub resource: SyncResource,
    pub boot_id: String,
    pub version: u64,
    pub deleted: bool,
    pub urgent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationCursor {
    pub journal_id: String,
    pub sequence: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEvent {
    Ready {
        journal_id: String,
        head: u64,
        after: u64,
    },
    Sync {
        event: SyncEvent,
    },
    Completion {
        journal_id: String,
        sequence: u64,
        completion: super::TurnCompletion,
    },
}
