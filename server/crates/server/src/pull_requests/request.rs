use crate::git_platform_providers::GitPlatformQuery;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestListState {
    Open,
    All,
}
#[derive(Deserialize)]
pub struct PullRequestListQuery {
    #[serde(flatten)]
    pub target: GitPlatformQuery,
    pub state: Option<PullRequestListState>,
}
#[derive(Deserialize)]
pub struct DiffQuery {
    #[serde(flatten)]
    pub target: GitPlatformQuery,
    pub path: String,
    #[serde(default)]
    pub patch_only: bool,
}
