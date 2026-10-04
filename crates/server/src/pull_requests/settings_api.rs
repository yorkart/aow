use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::{
    Provider, PullRequestError, ReviewQuery, Settings, Target, invalid_json, run_adapter,
    validate_settings,
};
use crate::{AppState, HttpError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/review-providers",
            get(settings).put(save_settings),
        )
        .route("/api/aow/review-providers/test", post(test_provider))
        .route("/api/review-targets", get(targets))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
}
async fn settings(State(state): State<AppState>) -> std::result::Result<Json<Settings>, HttpError> {
    Ok(Json(state.review_providers.settings()?))
}
async fn save_settings(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> std::result::Result<Json<Settings>, HttpError> {
    let manager = state.review_providers.clone();
    let settings = tokio::task::spawn_blocking(move || manager.save(settings))
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))??;
    Ok(Json(settings))
}
async fn test_provider(
    State(state): State<AppState>,
    Json(provider): Json<Provider>,
) -> std::result::Result<Json<Value>, HttpError> {
    validate_settings(&Settings {
        revision: 0,
        providers: vec![provider.clone()],
    })?;
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))?;
    let result = run_adapter(
        &provider,
        json!({"version":2,"operation":"describe","repository":null,"params":{}}),
        None,
        &paths,
        None,
    )
    .await?;
    let operations = result
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_json("describe requires operations"))?;
    if !["list", "detail", "diff"]
        .iter()
        .all(|op| operations.iter().any(|v| v.as_str() == Some(op)))
    {
        return Err(invalid_json("describe must implement list, detail, diff").into());
    }
    Ok(Json(result))
}
async fn targets(
    State(state): State<AppState>,
    Query(query): Query<ReviewQuery>,
) -> std::result::Result<Json<Vec<Target>>, HttpError> {
    Ok(Json(
        state
            .review_providers
            .targets(
                &query.repo,
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| PullRequestError::Command(e.to_string()))?,
            )
            .await?,
    ))
}
