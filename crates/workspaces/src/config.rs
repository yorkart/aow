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
            "工作区路径包含控制字符"
        );
        match self.workspace_mode {
            WorkspaceMode::Existing => ensure!(
                self.workspace_path.is_absolute(),
                "请选择要使用的 Worktree，路径必须为绝对路径"
            ),
            WorkspaceMode::NewWorktree => ensure!(
                !self.base_branch.trim().is_empty()
                    && self.base_branch.len() <= 1024
                    && !self.base_branch.trim().starts_with('-')
                    && !self.base_branch.chars().any(char::is_control),
                "请选择有效的基准分支，长度不能超过 1024 字节"
            ),
            WorkspaceMode::Temporary => {}
        }
        Ok(())
    }
}
