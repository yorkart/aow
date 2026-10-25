use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};

use super::{DiffQuery, PullRequestListQuery, positive, safe_path};
use crate::git_platform_providers::{GitPlatformError, GitPlatformQuery};
use crate::{AppState, HttpError};

pub(crate) async fn my_pull_requests(
    State(state): State<AppState>,
    Query(query): Query<PullRequestListQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let params = match query.state {
        Some(state) => serde_json::json!({ "state": state }),
        None => serde_json::json!({}),
    };
    Ok(Json(
        super::call(
            &state.git_platform_providers,
            &query.target,
            "pull_requests.list",
            params,
            &state
                .aow
                .execution_path()
                .await
                .map_err(|e| GitPlatformError::Command(e.to_string()))?,
        )
        .await?,
    ))
}

pub(crate) async fn my_pull_request_detail(
    State(state): State<AppState>,
    AxumPath(number): AxumPath<u64>,
    Query(query): Query<GitPlatformQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    positive(number)?;
    Ok(Json(
        super::call(
            &state.git_platform_providers,
            &query,
            "pull_requests.detail",
            serde_json::json!({"number":number}),
            &state
                .aow
                .execution_path()
                .await
                .map_err(|e| GitPlatformError::Command(e.to_string()))?,
        )
        .await?,
    ))
}

pub(crate) async fn my_pull_request_diff(
    State(state): State<AppState>,
    AxumPath(number): AxumPath<u64>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    positive(number)?;
    safe_path(&query.path)?;
    Ok(Json(
        super::call(
            &state.git_platform_providers,
            &query.target,
            "pull_requests.diff",
            serde_json::json!({"number":number,"path":query.path,"patch_only":query.patch_only}),
            &state
                .aow
                .execution_path()
                .await
                .map_err(|e| GitPlatformError::Command(e.to_string()))?,
        )
        .await?,
    ))
}
