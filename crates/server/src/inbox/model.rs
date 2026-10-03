use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Label {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Item {
    pub id: String,
    pub markdown: String,
    pub project_id: Option<String>,
    pub label_ids: Vec<String>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Document {
    pub version: u32,
    pub revision: u64,
    pub labels: Vec<Label>,
    // Array order is the user-defined order; filters never rewrite it.
    pub items: Vec<Item>,
    #[serde(default)]
    pub captures: BTreeMap<String, (String, String)>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            items: vec![],
            captures: BTreeMap::new(),
            labels: [
                ("todo", "TODO", "#94a3b8"),
                ("in-progress", "InProgress", "#60a5fa"),
                ("review", "Review", "#c4b5fd"),
                ("done", "Done", "#86cfa5"),
            ]
            .map(|(id, name, color)| Label {
                id: id.into(),
                name: name.into(),
                color: color.into(),
            })
            .into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum RunPhase {
    Starting,
    Submitted,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Run {
    pub id: String,
    pub item_id: String,
    pub request_key: String,
    #[serde(default)]
    pub execution_fingerprint: String,
    pub item_revision: u64,
    pub agent: String,
    pub project_id: String,
    pub cwd: String,
    #[serde(default = "existing_workspace_mode")]
    pub workspace_mode: aow_workspaces::WorkspaceMode,
    pub markdown: String,
    pub phase: RunPhase,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub(super) struct Snapshot {
    pub revision: u64,
    pub labels: Vec<Label>,
    pub items: Vec<Item>,
    pub executions: Vec<Run>,
    pub comment_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum AuthorType {
    Human,
    Ai,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CommentAuthor {
    #[serde(rename = "type")]
    pub kind: AuthorType,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Comment {
    pub id: String,
    pub author: CommentAuthor,
    pub created_at: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddComment {
    pub request_key: String,
    pub author: CommentAuthor,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    pub request_key: String,
    pub markdown: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Update {
    pub expected_revision: u64,
    pub markdown: String,
    pub project_id: Option<String>,
    pub label_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Revision {
    pub expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reorder {
    pub expected_revision: u64,
    pub item_id: String,
    pub before_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LabelsUpdate {
    pub expected_revision: u64,
    pub labels: Vec<Label>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execute {
    pub expected_revision: u64,
    pub request_key: String,
    pub agent: String,
    #[serde(default)]
    pub append_prompt: String,
    #[serde(flatten)]
    pub workspace: aow_workspaces::WorkspaceConfig,
}

fn existing_workspace_mode() -> aow_workspaces::WorkspaceMode {
    aow_workspaces::WorkspaceMode::Existing
}

impl Execute {
    pub(super) fn fingerprint(&self) -> Result<String, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| format!("{:x}", md5::compute(bytes)))
    }

    pub(super) fn final_task(&self, requirement: &str) -> String {
        if self.append_prompt.is_empty() {
            requirement.to_owned()
        } else {
            format!("{requirement}\n\n{}", self.append_prompt)
        }
    }
}
