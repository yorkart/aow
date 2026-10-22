use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use super::{GitPlatformError, GitPlatformProvider, Settings, Target, invalid_json};
use crate::{AppState, HttpError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(super::schema::PUBLIC_PATH, get(super::schema::get))
        .route(
            "/api/aow/git-platform-providers",
            get(settings).put(save_settings),
        )
        .route("/api/aow/git-platform-providers/test", post(test_provider))
        .route("/api/git-platform-targets", get(targets))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
}
async fn settings(State(state): State<AppState>) -> std::result::Result<Json<Settings>, HttpError> {
    Ok(Json(state.git_platform_providers.settings()?))
}
async fn save_settings(
    State(state): State<AppState>,
    Json(mut settings): Json<Settings>,
) -> std::result::Result<Json<Settings>, HttpError> {
    let manager = state.git_platform_providers.clone();
    let previous = manager.settings()?;
    if previous.revision != settings.revision {
        return Err(GitPlatformError::Conflict.into());
    }
    if settings.providers.len() > 32 {
        return Err(invalid_json("at most 32 providers are allowed").into());
    }
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| GitPlatformError::Command(e.to_string()))?;
    for provider in &mut settings.providers {
        let metadata = super::description::describe(provider, &paths).await?;
        provider.name = metadata.name;
        provider.hosts = metadata.hosts;
    }
    let settings = aow_i18n::spawn_blocking(move || manager.save(settings))
        .await
        .map_err(|e| GitPlatformError::Command(e.to_string()))??;
    Ok(Json(settings))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    id: String,
    script: String,
}

async fn test_provider(
    State(state): State<AppState>,
    Json(upload): Json<Upload>,
) -> std::result::Result<Json<Value>, HttpError> {
    let provider = GitPlatformProvider {
        id: upload.id,
        script: upload.script,
        name: String::new(),
        hosts: vec![],
        enabled: true,
    };
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| GitPlatformError::Command(e.to_string()))?;
    let result = super::description::describe(&provider, &paths).await?;
    Ok(Json(serde_json::to_value(result).map_err(invalid_json)?))
}
#[derive(Deserialize)]
struct TargetQuery {
    repo: String,
    #[serde(default)]
    operations: String,
}
async fn targets(
    State(state): State<AppState>,
    Query(query): Query<TargetQuery>,
) -> std::result::Result<Json<Vec<Target>>, HttpError> {
    Ok(Json(
        state
            .git_platform_providers
            .targets(
                &query.repo,
                &query
                    .operations
                    .split(',')
                    .filter(|op| !op.is_empty())
                    .collect::<Vec<_>>(),
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| GitPlatformError::Command(e.to_string()))?,
            )
            .await?,
    ))
}
