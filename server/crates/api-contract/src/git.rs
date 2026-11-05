//! Server enriches node-owned Git metadata with provider web links.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitDetail {
    #[serde(flatten)]
    pub commit: aow_runtime_contract::workspace::GitCommitDetail,
    pub remote_url: Option<String>,
    pub commit_url: Option<String>,
}
