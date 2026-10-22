use std::path::PathBuf;

use anyhow::{Result, ensure};
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

impl WorkspaceConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self
                .workspace_path
                .to_string_lossy()
                .chars()
                .any(char::is_control),
            aow_i18n::message("workspaces.workspace_path_contains_control_characters", &[])
        );
        match self.workspace_mode {
            WorkspaceMode::Existing => ensure!(
                self.workspace_path.is_absolute(),
                aow_i18n::message("workspaces.choose_a_worktree_with_an_absolute_path", &[])
            ),
            WorkspaceMode::NewWorktree => ensure!(
                !self.base_branch.trim().is_empty()
                    && self.base_branch.len() <= 1024
                    && !self.base_branch.trim().starts_with('-')
                    && !self.base_branch.chars().any(char::is_control),
                aow_i18n::message(
                    "workspaces.choose_a_valid_base_branch_up_to_1024_bytes",
                    &[]
                )
            ),
            WorkspaceMode::Temporary => {}
            WorkspaceMode::Dynamic => ensure!(
                self.workspace_path.as_os_str().is_empty(),
                aow_i18n::message(
                    "workspaces.dynamic_workspace_directory_must_be_provided_at_runtime",
                    &[]
                )
            ),
        }
        Ok(())
    }
}
