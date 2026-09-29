use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    sources::{RequirementSource, SourceSettings},
    store::{identifier, invalid, text},
};
use crate::{
    AppState, HttpError,
    pull_requests::{PullRequestError, ReviewQuery},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/sources/{project}", get(settings).put(save))
        .route("/sources/{project}/targets", get(targets))
        .route("/sources/{project}/issues", post(issues))
        .route("/sources/{project}/labels", get(labels))
}

fn repository(state: &AppState, project: &str) -> Result<String, HttpError> {
    identifier(project)?;
    Ok(state
        .aow
        .registered_project(project)
        .map_err(crate::aow::aow_http_error)?
        .repo_path)
}

async fn settings(
    State(state): State<AppState>,
    Path(project): Path<String>,
) -> Result<Json<SourceSettings>, HttpError> {
    repository(&state, &project)?;
    Ok(Json(state.tasks.sources(&project)?))
}

async fn save(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(settings): Json<SourceSettings>,
) -> Result<Json<SourceSettings>, HttpError> {
    let repo = repository(&state, &project)?;
    settings.validate()?;
    for source in &settings.sources {
        if let RequirementSource::RepositoryIssues {
            enabled: true,
            provider: Some(provider),
            remote: Some(remote),
        } = source
        {
            let paths = state
                .aow
                .execution_path()
                .await
                .map_err(|e| PullRequestError::Command(e.to_string()))?;
            let targets = state.review_providers.targets(&repo, &paths).await?;
            if !targets
                .iter()
                .any(|t| &t.provider == provider && &t.remote == remote)
            {
                return Err(invalid("所选仓库来源已不可用，请刷新需求源配置"));
            }
        }
    }
    let store = state.tasks.clone();
    Ok(Json(
        tokio::task::spawn_blocking(move || store.save_sources(&project, settings))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))??,
    ))
}

async fn targets(
    State(state): State<AppState>,
    Path(project): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let repo = repository(&state, &project)?;
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))?;
    Ok(Json(
        json!({"repository": repo, "targets": state.review_providers.targets(&repo, &paths).await?}),
    ))
}

async fn query(
    state: AppState,
    project: String,
    operation: &str,
    params: Value,
) -> Result<Json<Value>, HttpError> {
    let repo = repository(&state, &project)?;
    let settings = state.tasks.sources(&project)?;
    let (provider, remote) = settings
        .sources
        .into_iter()
        .find_map(|source| match source {
            RequirementSource::RepositoryIssues {
                enabled: true,
                provider,
                remote,
            } => Some((provider, remote)),
            _ => None,
        })
        .ok_or_else(|| invalid("Issue 需求源已停用"))?;
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))?;
    let query = ReviewQuery {
        repo,
        provider,
        remote,
    };
    Ok(Json(
        state
            .review_providers
            .call(&query, operation, params, &paths)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueQuery {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default = "open")]
    state: String,
}
fn open() -> String {
    "open".into()
}

async fn issues(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(input): Json<IssueQuery>,
) -> Result<Json<Value>, HttpError> {
    if !matches!(input.state.as_str(), "open" | "closed" | "all") || input.labels.len() > 100 {
        return Err(invalid(
            "Issue state must be open, closed or all; at most 100 labels",
        ));
    }
    for label in &input.labels {
        text(label, 256)?;
    }
    query(
        state,
        project,
        "issues",
        json!({"state": input.state, "labels": input.labels}),
    )
    .await
}

async fn labels(
    State(state): State<AppState>,
    Path(project): Path<String>,
) -> Result<Json<Value>, HttpError> {
    query(state, project, "issue_labels", json!({})).await
}
