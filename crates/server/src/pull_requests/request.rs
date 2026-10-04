use serde::{Deserialize, Serialize};

pub struct CommitLinks {
    pub remote_url: Option<String>,
    pub commit_url: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub remote: String,
    pub host: String,
    pub repository: String,
    pub provider: String,
    pub provider_name: String,
}
#[derive(Deserialize)]
pub struct ReviewQuery {
    pub repo: String,
    pub provider: Option<String>,
    pub remote: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestListState {
    Open,
    All,
}
#[derive(Deserialize)]
pub struct PullRequestListQuery {
    #[serde(flatten)]
    pub target: ReviewQuery,
    pub state: Option<PullRequestListState>,
}
#[derive(Deserialize)]
pub struct DiffQuery {
    #[serde(flatten)]
    pub target: ReviewQuery,
    pub path: String,
    #[serde(default)]
    pub patch_only: bool,
}
