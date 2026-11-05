use super::*;
use crate::settings::persistence::{REGISTRY_VERSION, RegistryDocument};
use std::collections::BTreeMap;
#[derive(Default)]
pub(crate) struct RegistryState {
    pub(crate) projects: Vec<StoredProject>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredProject {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) registered_path: String,
    #[serde(default)]
    pub(crate) common_git_dir: String,
    pub(crate) notes_path: String,
    #[serde(default)]
    pub(crate) notes_identity: Option<PathBuf>,
    #[serde(default)]
    pub(crate) notes_custom: bool,
    #[serde(default)]
    pub(crate) builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) avatar_url: Option<String>,
    #[serde(default)]
    pub(crate) worktree_colors: BTreeMap<String, WorktreeColor>,
    #[serde(default)]
    pub(crate) worktree_icons: BTreeMap<String, WorktreeIcon>,
    #[serde(default)]
    pub(crate) task_worktree: TaskWorktreePolicy,
}

impl ProjectService {
    pub(crate) fn persist_projects(&self, projects: &[StoredProject]) -> Result<(), ServiceError> {
        if let Some(path) = &self.inner.projects_path {
            crate::settings::persistence::save_configuration(
                self.inner.config.as_ref(),
                path,
                &RegistryDocument {
                    version: REGISTRY_VERSION,
                    items: projects.to_vec(),
                },
            )?;
        }
        Ok(())
    }
}
