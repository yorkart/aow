use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::aow) struct AowSettings {
    pub(in crate::aow) notes_base: String,
    #[serde(default)]
    pub(in crate::aow) node_addresses: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) editor: EditorSettings,
    #[serde(default)]
    pub(in crate::aow) execution_path: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub(in crate::aow) pinned_worktrees: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) pinned_worktrees_revision: u64,
    #[serde(default)]
    pub(in crate::aow) pinned_directories: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) pinned_directories_revision: u64,
}

impl AowSettings {
    pub(in crate::aow) fn with_notes_base(notes_base: PathBuf) -> Self {
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
pub(in crate::aow) struct EditorSettings {
    #[serde(default)]
    pub(in crate::aow) word_wrap: bool,
}
