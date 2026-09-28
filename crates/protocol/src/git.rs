use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySummary {
    pub path: String,
    pub name: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitIgnoredPaths {
    pub repository: Option<String>,
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub repository: String,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFileStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub author: String,
    pub authored_at: String,
    pub subject: String,
    pub parents: Vec<String>,
    pub is_pushed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLog {
    pub repository: String,
    pub upstream: Option<String>,
    pub upstream_commit: Option<String>,
    pub commits: Vec<GitCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitFile {
    pub path: String,
    pub status: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitFiles {
    pub repository: String,
    pub commit: String,
    pub files: Vec<GitCommitFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitStats {
    pub files_changed: u32,
    pub insertions: u64,
    pub deletions: u64,
    pub binary_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitDetail {
    pub repository: String,
    pub id: String,
    pub short_id: String,
    pub author: GitIdentity,
    pub committer: GitIdentity,
    pub subject: String,
    pub body: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub stats: GitCommitStats,
    pub upstream: Option<String>,
    pub remote_name: Option<String>,
    /// Optional web links supplied by the configured repository Provider.
    pub remote_url: Option<String>,
    pub commit_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiff {
    pub repository: String,
    pub path: Option<String>,
    pub staged: bool,
    pub patch: String,
    pub truncated: bool,
    pub original: Option<String>,
    pub modified: Option<String>,
}
