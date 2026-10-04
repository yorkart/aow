use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};

use super::{DiffQuery, PullRequestError, PullRequestListQuery, ReviewQuery, positive, safe_path};
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
        state
            .review_providers
            .call(
                &query.target,
                "list",
                params,
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| PullRequestError::Command(e.to_string()))?,
            )
            .await?,
    ))
}

pub(crate) async fn my_pull_request_detail(
    State(state): State<AppState>,
    AxumPath(number): AxumPath<u64>,
    Query(query): Query<ReviewQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    positive(number)?;
    Ok(Json(
        state
            .review_providers
            .call(
                &query,
                "detail",
                serde_json::json!({"number":number}),
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| PullRequestError::Command(e.to_string()))?,
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
    Ok(Json(state.review_providers.call(&query.target, "diff", serde_json::json!({"number":number,"path":query.path,"patch_only":query.patch_only}), &state.aow.execution_path().await.map_err(|e| PullRequestError::Command(e.to_string()))?).await?))
}
