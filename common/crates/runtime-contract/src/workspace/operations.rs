use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", content = "arguments", rename_all = "snake_case")]
pub enum WorkspaceRequest {
    Home {},
    TerminalDefaults {},
    Account {},
    TemporaryNote {
        root: PathBuf,
        extension: String,
    },
    List {
        path: PathBuf,
    },
    Read {
        path: PathBuf,
    },
    Canonicalize {
        path: PathBuf,
    },
    Metadata {
        path: PathBuf,
        follow: bool,
    },
    Create {
        parent: PathBuf,
        name: String,
        kind: EntryKind,
    },
    MkdirAll {
        path: PathBuf,
    },
    Rename {
        path: PathBuf,
        name: String,
        file_only: bool,
    },
    Delete {
        path: PathBuf,
    },
    GitCommand {
        cwd: PathBuf,
        args: Vec<String>,
    },
    GitStatus {
        repo: PathBuf,
    },
    GitRepositories {
        root: PathBuf,
        depth: usize,
    },
    GitIgnored {
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
    GitPull {
        repo: PathBuf,
    },
    GitPush {
        repo: PathBuf,
    },
    GitLog {
        repo: PathBuf,
        limit: usize,
    },
    GitDiff {
        repo: PathBuf,
        path: Option<String>,
        staged: bool,
    },
    GitCommitDetail {
        repo: PathBuf,
        commit: String,
    },
    GitCommitFiles {
        repo: PathBuf,
        commit: String,
    },
    GitCommitDiff {
        repo: PathBuf,
        commit: String,
        path: String,
        original_path: Option<String>,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Empty {}
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
pub struct GitCommandResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceChanges {
    pub revision: u64,
    pub worktrees: u64,
    pub repositories: BTreeMap<PathBuf, u64>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PathResponse {
    pub path: PathBuf,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CanonicalPath {
    pub path: PathBuf,
    pub directory: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalDefaults {
    pub shell: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Note {
    pub path: PathBuf,
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CreatedEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub search: aow_protocol::FileSearchRequest,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub filename: String,
    pub script: String,
    pub request: serde_json::Value,
    pub cwd: Option<PathBuf>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClipboardImage {
    pub path: String,
    pub mime: String,
    pub size: u64,
    pub expires_at: String,
}
