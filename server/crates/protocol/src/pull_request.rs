use serde::{Deserialize, Serialize};

/// Platform-independent review data returned by a configured provider adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyPullRequests {
    pub repository: String,
    pub current_branch: String,
    pub current_user: PullRequestUser,
    pub pull_requests: Vec<PullRequestSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestSummary {
    pub number: u64,
    pub status: String,
    pub draft: bool,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestDetail {
    #[serde(flatten)]
    pub summary: PullRequestSummary,
    pub description: String,
    pub changes_count: u32,
    pub commits_count: u32,
    pub review_status: String,
    pub check_summary_status: String,
    pub mergeable: Option<bool>,
    pub reviewers: Vec<PullRequestUser>,
    pub checks: Vec<PullRequestCheck>,
    pub unresolved_threads: Vec<PullRequestThread>,
    pub files: Vec<PullRequestFile>,
    pub author: Option<PullRequestUser>,
    pub labels: Vec<String>,
    pub merge_checks: Vec<PullRequestMergeCheck>,
    pub threads: Vec<PullRequestThread>,
    pub warnings: Vec<String>,
    pub diverged_commits_count: u64,
    pub milestone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestMergeCheck {
    pub name: String,
    pub passed: Option<bool>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestCheck {
    pub id: String,
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub details_url: Option<String>,
    pub description: String,
    pub text: String,
    pub required: bool,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestThread {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub status: String,
    pub author: String,
    pub body: String,
    pub updated_at: Option<String>,
    pub comments: Vec<PullRequestComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestFile {
    pub path: String,
    pub change_type: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestDiff {
    pub repository: String,
    pub number: u64,
    pub path: String,
    pub original_path: Option<String>,
    pub original: Option<String>,
    pub modified: Option<String>,
    #[serde(default)]
    pub patch: Option<String>,
    pub binary: bool,
    pub truncated: bool,
}
