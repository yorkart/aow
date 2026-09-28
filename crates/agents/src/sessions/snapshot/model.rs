use super::tool_details::ToolDetails;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("agent session is no longer available")]
    NotFound,
    #[error("invalid agent session transcript: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSessionSnapshot {
    pub(super) session_id: String,
    pub(super) agent: &'static str,
    pub(super) title: String,
    pub(super) captured_at: String,
    pub(super) status: &'static str,
    pub(super) turns: Vec<SnapshotTurn>,
    pub(super) truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct SnapshotTurn {
    pub(super) id: String,
    pub(super) status: &'static str,
    pub(super) user: SnapshotMessage,
    #[serde(rename = "final")]
    pub(super) final_message: Option<SnapshotMessage>,
    pub(super) activities: Vec<SnapshotActivity>,
    pub(super) activities_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct SnapshotMessage {
    pub(super) text: String,
    pub(super) timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct SnapshotActivity {
    pub(super) id: String,
    pub(super) kind: &'static str,
    // Public progress text or the tool name. Tool payloads live in details;
    // reasoning records are never part of the snapshot.
    pub(super) text: String,
    pub(super) timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) status: Option<&'static str>,
    // Semantic categories from parsed commands, without command text or paths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) actions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) details: Option<ToolDetails>,
}

impl SnapshotActivity {
    pub(super) fn merge_details(&mut self, details: Option<ToolDetails>) {
        if let Some(details) = details {
            self.details
                .get_or_insert_with(ToolDetails::default)
                .merge(details);
        }
        if self
            .details
            .as_ref()
            .and_then(|details| details.exit_code)
            .is_some_and(|code| code != 0)
        {
            self.status = Some("failed");
        }
    }
}
