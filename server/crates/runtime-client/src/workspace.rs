//! Typed node workspace capabilities and streaming file transport.
use crate::{RuntimeClient, RuntimeError, RuntimeSocket};
use aow_runtime_contract::workspace::*;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
impl RuntimeClient {
    async fn workspace_request<R: DeserializeOwned>(
        &self,
        request: WorkspaceRequest,
    ) -> Result<R, RuntimeError> {
        self.post_json("/v1/workspace/operations", &request).await
    }
    pub async fn workspace_home(&self) -> Result<PathBuf, RuntimeError> {
        let result: PathResponse = self.workspace_request(WorkspaceRequest::Home {}).await?;
        Ok(result.path)
    }
    pub async fn workspace_terminal_defaults(&self) -> Result<String, RuntimeError> {
        let result: TerminalDefaults = self
            .workspace_request(WorkspaceRequest::TerminalDefaults {})
            .await?;
        Ok(result.shell)
    }
    pub async fn workspace_account(&self) -> Result<String, RuntimeError> {
        let result: Account = self.workspace_request(WorkspaceRequest::Account {}).await?;
        Ok(result.name)
    }
    pub async fn workspace_temporary_note(
        &self,
        root: &Path,
        extension: &str,
    ) -> Result<Note, RuntimeError> {
        self.workspace_request(WorkspaceRequest::TemporaryNote {
            root: root.into(),
            extension: extension.into(),
        })
        .await
    }
    pub async fn workspace_list(
        &self,
        path: &Path,
    ) -> Result<aow_protocol::DirectoryListing, RuntimeError> {
        self.workspace_request(WorkspaceRequest::List { path: path.into() })
            .await
    }
    pub async fn workspace_read(
        &self,
        path: &Path,
    ) -> Result<aow_protocol::TextFile, RuntimeError> {
        self.workspace_request(WorkspaceRequest::Read { path: path.into() })
            .await
    }
    pub async fn workspace_canonicalize(
        &self,
        path: &Path,
    ) -> Result<(PathBuf, bool), RuntimeError> {
        let result: CanonicalPath = self
            .workspace_request(WorkspaceRequest::Canonicalize { path: path.into() })
            .await?;
        Ok((result.path, result.directory))
    }
    pub async fn workspace_metadata(
        &self,
        path: &Path,
        follow: bool,
    ) -> Result<NodeMetadata, RuntimeError> {
        self.workspace_request(WorkspaceRequest::Metadata {
            path: path.into(),
            follow,
        })
        .await
    }
    pub async fn workspace_create(
        &self,
        parent: &Path,
        name: &str,
        kind: EntryKind,
    ) -> Result<CreatedEntry, RuntimeError> {
        self.workspace_request(WorkspaceRequest::Create {
            parent: parent.into(),
            name: name.into(),
            kind,
        })
        .await
    }
    pub async fn workspace_mkdir_all(&self, path: &Path) -> Result<(), RuntimeError> {
        let _: Empty = self
            .workspace_request(WorkspaceRequest::MkdirAll { path: path.into() })
            .await?;
        Ok(())
    }
    pub async fn workspace_rename(
        &self,
        path: &Path,
        name: &str,
        file_only: bool,
    ) -> Result<aow_protocol::RenameResult, RuntimeError> {
        self.workspace_request(WorkspaceRequest::Rename {
            path: path.into(),
            name: name.into(),
            file_only,
        })
        .await
    }
    pub async fn workspace_delete(&self, path: &Path) -> Result<(), RuntimeError> {
        let _: Empty = self
            .workspace_request(WorkspaceRequest::Delete { path: path.into() })
            .await?;
        Ok(())
    }
    pub async fn workspace_git(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<GitCommandResult, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitCommand {
            cwd: cwd.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        })
        .await
    }
    pub async fn workspace_git_status(
        &self,
        repo: &Path,
    ) -> Result<aow_protocol::GitStatus, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitStatus { repo: repo.into() })
            .await
    }
    pub async fn workspace_git_repositories(
        &self,
        root: &Path,
        depth: usize,
    ) -> Result<Vec<aow_protocol::RepositorySummary>, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitRepositories {
            root: root.into(),
            depth,
        })
        .await
    }
    pub async fn workspace_git_ignored(
        &self,
        root: &Path,
        paths: &[PathBuf],
    ) -> Result<aow_protocol::GitIgnoredPaths, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitIgnored {
            root: root.into(),
            paths: paths.to_vec(),
        })
        .await
    }
    pub async fn workspace_git_pull(&self, repo: &Path) -> Result<(), RuntimeError> {
        let _: Empty = self
            .workspace_request(WorkspaceRequest::GitPull { repo: repo.into() })
            .await?;
        Ok(())
    }
    pub async fn workspace_git_push(&self, repo: &Path) -> Result<(), RuntimeError> {
        let _: Empty = self
            .workspace_request(WorkspaceRequest::GitPush { repo: repo.into() })
            .await?;
        Ok(())
    }
    pub async fn workspace_git_log(
        &self,
        repo: &Path,
        limit: usize,
    ) -> Result<aow_protocol::GitLog, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitLog {
            repo: repo.into(),
            limit,
        })
        .await
    }
    pub async fn workspace_git_diff(
        &self,
        repo: &Path,
        path: Option<&str>,
        staged: bool,
    ) -> Result<aow_protocol::GitDiff, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitDiff {
            repo: repo.into(),
            path: path.map(str::to_owned),
            staged,
        })
        .await
    }
    pub async fn workspace_git_commit_detail(
        &self,
        repo: &Path,
        commit: &str,
    ) -> Result<aow_protocol::GitCommitDetail, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitCommitDetail {
            repo: repo.into(),
            commit: commit.into(),
        })
        .await
    }
    pub async fn workspace_git_commit_files(
        &self,
        repo: &Path,
        commit: &str,
    ) -> Result<aow_protocol::GitCommitFiles, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitCommitFiles {
            repo: repo.into(),
            commit: commit.into(),
        })
        .await
    }
    pub async fn workspace_git_commit_diff(
        &self,
        repo: &Path,
        commit: &str,
        path: &str,
        original_path: Option<&str>,
    ) -> Result<aow_protocol::GitDiff, RuntimeError> {
        self.workspace_request(WorkspaceRequest::GitCommitDiff {
            repo: repo.into(),
            commit: commit.into(),
            path: path.into(),
            original_path: original_path.map(str::to_owned),
        })
        .await
    }
    pub async fn workspace_clipboard(
        &self,
        body: reqwest::Body,
    ) -> Result<ClipboardImage, RuntimeError> {
        let response = self
            .stream_request(
                http::Method::POST,
                "/v1/workspace/clipboard",
                http::HeaderMap::new(),
                Some(body),
            )
            .await?;
        Self::json(response.status(), response.bytes().await?)
    }
    pub async fn search_workspace(
        &self,
        request: &SearchRequest,
    ) -> Result<aow_protocol::FileSearchResult, RuntimeError> {
        self.post_json("/v1/workspace/search", request).await
    }
    pub async fn cancel_workspace_search(&self, id: &str) -> Result<(), RuntimeError> {
        self.delete(&format!(
            "/v1/workspace/search/{}",
            percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC)
        ))
        .await
    }
    pub async fn workspace_provider(
        &self,
        request: &ProviderRequest,
    ) -> Result<serde_json::Value, RuntimeError> {
        self.post_json("/v1/workspace/provider", request).await
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
