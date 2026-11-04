//! Workspace policy belongs to a conversation, never to a prompt execution.
use super::{TaskDb, TaskRecord};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationPolicy {
    pub workspace: String,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
    pub target_revision: Option<String>,
}
impl From<&TaskRecord> for ConversationPolicy {
    fn from(task: &TaskRecord) -> Self {
        Self {
            workspace: task.workspace.clone(),
            workspace_intent: task.workspace_intent.clone(),
            target_task_id: task.target_task_id.clone(),
            target_revision: task.target_revision.clone(),
        }
    }
}
impl TaskDb {
    pub fn conversation_policy(&self, thread: &str) -> Result<Option<ConversationPolicy>> {
        let data: Option<String> = self
            .lock()?
            .query_row(
                "SELECT payload FROM task_conversation_policies WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn owns_workspace(&self, task: &str, workspace: &str) -> Result<bool> {
        Ok(self.lock()?.query_row("SELECT EXISTS(SELECT 1 FROM task_conversation_policies WHERE task_id=?1 AND json_extract(payload,'$.workspace')=?2)", params![task,workspace], |row| row.get(0))?)
    }
}
