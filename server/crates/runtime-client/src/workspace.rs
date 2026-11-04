//! Node workspace operations and streaming file transport.
use crate::{RuntimeClient, RuntimeError, RuntimeSocket};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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
impl RuntimeClient {
    pub async fn workspace_clipboard(&self, body: reqwest::Body) -> Result<Value, RuntimeError> {
        let response = self
            .stream_request(
                http::Method::POST,
                "/v1/workspace/clipboard",
                http::HeaderMap::new(),
                Some(body),
            )
            .await?;
        let status = response.status();
        Self::json(status, response.bytes().await?)
    }

    pub async fn workspace_operation<R: DeserializeOwned>(
        &self,
        operation: &str,
        arguments: Value,
    ) -> Result<R, RuntimeError> {
        self.post_json(
            "/v1/workspace/operations",
            &json!({"operation":operation,"arguments":arguments}),
        )
        .await
    }
    pub async fn workspace_metadata(
        &self,
        path: &Path,
        follow: bool,
    ) -> Result<NodeMetadata, RuntimeError> {
        self.workspace_operation("metadata", json!({"path":path,"follow":follow}))
            .await
    }
    pub async fn workspace_canonicalize(
        &self,
        path: &Path,
    ) -> Result<(PathBuf, bool), RuntimeError> {
        #[derive(Deserialize)]
        struct Canonical {
            path: PathBuf,
            directory: bool,
        }
        let result: Canonical = self
            .workspace_operation("canonicalize", json!({"path":path}))
            .await?;
        Ok((result.path, result.directory))
    }
    pub async fn workspace_home(&self) -> Result<PathBuf, RuntimeError> {
        #[derive(Deserialize)]
        struct Home {
            path: PathBuf,
        }
        let result: Home = self.workspace_operation("home", json!({})).await?;
        Ok(result.path)
    }
    pub async fn workspace_git(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<GitCommandResult, RuntimeError> {
        self.workspace_operation("git_command", json!({"cwd":cwd,"args":args}))
            .await
    }
    pub async fn watch_workspaces(&self) -> Result<RuntimeSocket, RuntimeError> {
        self.websocket("/v1/workspace/watch").await
    }
    pub async fn workspace_transfer(
        &self,
        method: http::Method,
        path: &Path,
        keep_both: bool,
        attachment: bool,
        headers: &http::HeaderMap,
        body: Option<reqwest::Body>,
    ) -> Result<reqwest::Response, RuntimeError> {
        let mut query = reqwest::Url::parse("http://workspace/").expect("static workspace URL");
        query
            .query_pairs_mut()
            .append_pair("path", &path.to_string_lossy())
            .append_pair("keep_both", if keep_both { "true" } else { "false" })
            .append_pair("attachment", if attachment { "true" } else { "false" });
        let mut forwarded = http::HeaderMap::new();
        for name in [
            http::header::RANGE,
            http::header::IF_MATCH,
            http::header::CONTENT_TYPE,
            http::header::CONTENT_LENGTH,
        ] {
            if let Some(value) = headers.get(&name) {
                forwarded.insert(name, value.clone());
            }
        }
        self.stream_request(
            method,
            &format!("/v1/workspace/raw?{}", query.query().unwrap()),
            forwarded,
            body,
        )
        .await
    }
}
