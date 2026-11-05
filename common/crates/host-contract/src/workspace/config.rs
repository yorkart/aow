use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    #[default]
    NewWorktree,
    Existing,
    Temporary,
    /// A manual task template whose directory must be supplied for each run.
    Dynamic,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub workspace_mode: WorkspaceMode,
    #[serde(default)]
    pub workspace_path: PathBuf,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

fn default_base_branch() -> String {
    "HEAD".into()
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            workspace_mode: WorkspaceMode::NewWorktree,
            workspace_path: PathBuf::new(),
            base_branch: default_base_branch(),
        }
    }
}

#[derive(Debug)]
pub struct ValidationError(pub &'static str);
impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ValidationError {}
impl WorkspaceConfig {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let key = if self
            .workspace_path
            .to_string_lossy()
            .chars()
            .any(char::is_control)
        {
            Some("workspaces.workspace_path_contains_control_characters")
        } else {
            match self.workspace_mode {
                WorkspaceMode::Existing if !self.workspace_path.is_absolute() => {
                    Some("workspaces.choose_a_worktree_with_an_absolute_path")
                }
                WorkspaceMode::NewWorktree
                    if self.base_branch.trim().is_empty()
                        || self.base_branch.len() > 1024
                        || self.base_branch.trim().starts_with('-')
                        || self.base_branch.chars().any(char::is_control) =>
                {
                    Some("workspaces.choose_a_valid_base_branch_up_to_1024_bytes")
                }
                WorkspaceMode::Dynamic if !self.workspace_path.as_os_str().is_empty() => {
                    Some("workspaces.dynamic_workspace_directory_must_be_provided_at_runtime")
                }
                _ => None,
            }
        };
        key.map_or(Ok(()), |key| Err(ValidationError(key)))
    }
}
