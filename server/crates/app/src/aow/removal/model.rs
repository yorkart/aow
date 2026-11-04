use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::aow) enum RemovalStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl RemovalStatus {
    pub(super) fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::aow) struct RemovalRequest {
    pub path: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct BatchRequest {
    pub(super) items: Vec<RemovalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::aow) struct RemovalItem {
    pub(super) path: String,
    pub(super) force: bool,
    pub(super) status: RemovalStatus,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::aow) struct RemovalJob {
    pub(super) id: String,
    pub(super) project_id: String,
    pub(super) created_at: String,
    pub(super) items: Vec<RemovalItem>,
    #[serde(skip)]
    pub(super) finished_at: Option<std::time::Instant>,
}
