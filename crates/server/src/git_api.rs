use std::path::PathBuf;

use aow_git_service as git;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{AppState, HttpError, pull_requests};

#[derive(Deserialize)]
pub(crate) struct RepositoryQuery {
    root: String,
    #[serde(default = "default_depth")]
    depth: usize,
}

fn default_depth() -> usize {
    4
}

pub(crate) async fn git_repositories(
    Query(query): Query<RepositoryQuery>,
) -> Result<Json<Vec<aow_protocol::RepositorySummary>>, HttpError> {
    Ok(Json(
        git::discover_repositories(query.root, query.depth.min(8)).await?,
    ))
}

#[derive(Deserialize)]
pub(crate) struct IgnoredPathsRequest {
    root: String,
    paths: Vec<PathBuf>,
}

pub(crate) async fn git_ignored(
    Json(request): Json<IgnoredPathsRequest>,
) -> Result<Json<aow_protocol::GitIgnoredPaths>, HttpError> {
    Ok(Json(git::ignored_paths(request.root, request.paths).await?))
}

#[derive(Deserialize)]
pub(crate) struct RepoOnlyQuery {
    repo: String,
}

pub(crate) async fn git_status(
    Query(query): Query<RepoOnlyQuery>,
) -> Result<Json<aow_protocol::GitStatus>, HttpError> {
    Ok(Json(git::status(query.repo).await?))
}

pub(crate) async fn git_pull(Json(request): Json<RepoOnlyQuery>) -> Result<StatusCode, HttpError> {
    git::pull(request.repo).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn git_push(Json(request): Json<RepoOnlyQuery>) -> Result<StatusCode, HttpError> {
    git::push(request.repo).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(crate) struct LogQuery {
    repo: String,
    #[serde(default = "default_log_limit")]
    limit: usize,
}

fn default_log_limit() -> usize {
    100
}

pub(crate) async fn git_log(
    Query(query): Query<LogQuery>,
) -> Result<Json<aow_protocol::GitLog>, HttpError> {
    Ok(Json(git::log(query.repo, query.limit).await?))
}

#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    repo: String,
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

pub(crate) async fn git_diff(
    Query(query): Query<DiffQuery>,
) -> Result<Json<aow_protocol::GitDiff>, HttpError> {
    Ok(Json(
        git::diff(query.repo, query.path.as_deref(), query.staged).await?,
    ))
}

#[derive(Deserialize)]
pub(crate) struct CommitQuery {
    repo: String,
    commit: String,
}

pub(crate) async fn git_commit_detail(
    State(state): State<AppState>,
    Query(query): Query<CommitQuery>,
) -> Result<Json<aow_protocol::GitCommitDetail>, HttpError> {
    let mut detail = git::commit_detail(&query.repo, &query.commit).await?;
    if let Some(remote) = &detail.remote_name {
        // Links are optional enrichment. A slow or broken Provider must not
        // prevent local commit metadata from being displayed.
        let links = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let paths = state
                .aow
                .execution_path()
                .await
                .map_err(|e| pull_requests::PullRequestError::Command(e.to_string()))?;
            state
                .review_providers
                .commit_links(&query.repo, remote, &detail.id, &paths)
                .await
        })
        .await;
        match links {
            Ok(Ok(Some(links))) => {
                detail.remote_url = links.remote_url;
                detail.commit_url = links.commit_url;
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "commit links Provider failed"),
            Err(_) => tracing::warn!("commit links Provider timed out"),
        }
    }
    Ok(Json(detail))
}

pub(crate) async fn git_commit_files(
    Query(query): Query<CommitQuery>,
) -> Result<Json<aow_protocol::GitCommitFiles>, HttpError> {
    Ok(Json(git::commit_files(query.repo, &query.commit).await?))
}

#[derive(Deserialize)]
pub(crate) struct CommitDiffQuery {
    repo: String,
    commit: String,
    path: String,
    original_path: Option<String>,
}

pub(crate) async fn git_commit_diff(
    Query(query): Query<CommitDiffQuery>,
) -> Result<Json<aow_protocol::GitDiff>, HttpError> {
    Ok(Json(
        git::commit_diff(
            query.repo,
            &query.commit,
            &query.path,
            query.original_path.as_deref(),
        )
        .await?,
    ))
}
