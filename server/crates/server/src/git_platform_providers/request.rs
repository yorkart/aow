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
pub struct GitPlatformQuery {
    pub repo: String,
    pub provider: Option<String>,
    pub remote: Option<String>,
}
