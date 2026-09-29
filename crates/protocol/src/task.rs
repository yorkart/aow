//! A shared task board with user-defined states, independent of terminal execution state.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub project_id: String,
    pub revision: u64,
    pub title: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxSummary {
    pub id: String,
    pub project_id: String,
    pub revision: u64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub task_ids: Vec<String>,
}

impl From<&InboxItem> for InboxSummary {
    fn from(item: &InboxItem) -> Self {
        Self {
            id: item.id.clone(),
            project_id: item.project_id.clone(),
            revision: item.revision,
            title: item.title.clone(),
            created_at: item.created_at.clone(),
            updated_at: item.updated_at.clone(),
            task_ids: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxPage {
    pub items: Vec<InboxSummary>,
    pub total: usize,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStatus {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStatusChange {
    pub status_id: String,
    pub at: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutionPhase {
    Preparing,
    Ready,
    Submitting,
    Submitted,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardTask {
    pub id: String,
    pub revision: u64,
    pub inbox_id: String,
    pub status_id: String,
    pub title: String,
    pub description: String,
    pub project_id: String,
    pub cwd: String,
    pub agent: String,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
    pub execution: TaskExecutionPhase,
    pub error: Option<String>,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    pub history: Vec<TaskStatusChange>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskBoard {
    pub version: u32,
    pub status_revision: u64,
    pub statuses: Vec<TaskStatus>,
    pub tasks: Vec<BoardTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxCreate {
    /// Identifies a retry of this request; never used as the resource ID.
    pub request_key: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxUpdate {
    pub project_id: String,
    pub expected_revision: u64,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskRevision {
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStatusesWrite {
    pub request_key: String,
    pub expected_revision: u64,
    pub statuses: Vec<TaskStatusWrite>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStatusWrite {
    /// Existing status identity. Omit when creating a new status.
    pub id: Option<String>,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskWorktree {
    pub branch: String,
    pub base_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskConvert {
    pub request_key: String,
    pub expected_revision: u64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status_id: String,
    pub project_id: String,
    pub cwd: String,
    pub agent: String,
    pub worktree: Option<TaskWorktree>,
    #[serde(default)]
    pub start_now: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskMove {
    pub expected_revision: u64,
    pub status_id: String,
    #[serde(default)]
    pub reason: String,
}
