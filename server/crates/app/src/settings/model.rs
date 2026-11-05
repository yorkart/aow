use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AowSettings {
    pub(crate) notes_base: String,
    #[serde(default)]
    pub(crate) node_addresses: Vec<String>,
    #[serde(default)]
    pub(crate) editor: EditorSettings,
    #[serde(default)]
    pub(crate) execution_path: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub(crate) pinned_worktrees: Vec<String>,
    #[serde(default)]
    pub(crate) pinned_worktrees_revision: u64,
    #[serde(default)]
    pub(crate) pinned_directories: Vec<String>,
    #[serde(default)]
    pub(crate) pinned_directories_revision: u64,
}

impl AowSettings {
    pub(crate) fn with_notes_base(notes_base: PathBuf) -> Self {
        Self {
            notes_base: notes_base.to_string_lossy().into_owned(),
            node_addresses: Vec::new(),
            editor: EditorSettings::default(),
            execution_path: None,
            pinned_worktrees: Vec::new(),
            pinned_worktrees_revision: 0,
            pinned_directories: Vec::new(),
            pinned_directories_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct EditorSettings {
    #[serde(default)]
    pub(crate) word_wrap: bool,
}
