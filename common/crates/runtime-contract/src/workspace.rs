use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub exists: bool,
    #[serde(default)]
    pub file: bool,
    #[serde(default)]
    pub directory: bool,
    #[serde(default)]
    pub symlink: bool,
    #[serde(default)]
    pub size: u64,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GitCommandResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WorkspaceChanges {
    pub revision: u64,
    pub worktrees: u64,
    pub repositories: BTreeMap<PathBuf, u64>,
}
