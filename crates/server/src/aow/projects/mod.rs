mod model;
mod registry;

use super::*;

pub(super) use model::project_from_stored_strict;
use model::{project_from_stored, resolve_common_git_dir};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectSummary {
    pub(super) id: String,
    pub(super) name: String,
    pub(crate) repo_path: String,
}

impl From<&StoredProject> for ProjectSummary {
    fn from(project: &StoredProject) -> Self {
        Self {
            id: project.id.clone(),
            name: project.name.clone(),
            repo_path: project.registered_path.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Project {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) registered_path: String,
    pub(super) common_git_dir: String,
    pub(super) notes_path: String,
    pub(super) builtin: bool,
    pub(super) avatar_url: Option<String>,
    pub(super) worktrees: Vec<Worktree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterProjectRequest {
    pub(super) path: String,
    pub(super) name: Option<String>,
    pub(super) notes_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BindNotesRequest {
    pub(super) path: String,
}
