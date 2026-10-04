use super::{InboxStore, execution, model::*};
use crate::{AppState, HttpError};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};

pub(crate) fn routes() -> Router<AppState> {
    routes_at("/api/inbox")
}
pub(crate) fn cli_routes() -> Router<AppState> {
    routes_at("/v1/inbox")
}
fn routes_at(base: &str) -> Router<AppState> {
    Router::new()
        .route(base, get(snapshot))
        .route(&format!("{base}/items"), post(capture))
        .route(
            &format!("{base}/items/{{id}}"),
            get(item).put(update).delete(delete),
        )
        .route(
            &format!("{base}/items/{{id}}/comments"),
            get(comments).post(append_comment),
        )
        .route(
            &format!("{base}/items/{{id}}/execute"),
            post(execution::execute),
        )
        .route(&format!("{base}/order"), put(reorder))
        .route(&format!("{base}/labels"), put(labels))
}
pub(super) async fn blocking<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(InboxStore) -> Result<T, HttpError> + Send + 'static,
) -> Result<T, HttpError> {
    let store = state.inbox.clone();
    tokio::task::spawn_blocking(move || f(store))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?
}
async fn snapshot(State(state): State<AppState>) -> Result<Json<Snapshot>, HttpError> {
    blocking(&state, |store| store.snapshot()).await.map(Json)
}
async fn item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Item>, HttpError> {
    blocking(&state, move |store| store.item(&id))
        .await
        .map(Json)
}
async fn capture(
    State(state): State<AppState>,
    Json(input): Json<Capture>,
) -> Result<Json<Item>, HttpError> {
    let result = blocking(&state, |store| store.capture(input)).await;
    state.workspace_events.inbox_changed();
    result.map(Json)
}
async fn comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Comment>>, HttpError> {
    blocking(&state, move |store| store.comments(&id))
        .await
        .map(Json)
}
async fn append_comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AddComment>,
) -> Result<Json<Comment>, HttpError> {
    let result = blocking(&state, move |store| store.append_comment(&id, input)).await;
    state.workspace_events.inbox_changed();
    result.map(Json)
}
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Update>,
) -> Result<Json<Item>, HttpError> {
    if let Some(project) = &input.project_id {
        state
            .aow
            .registered_project(project)
            .map_err(crate::aow::aow_http_error)?;
    }
    let result = blocking(&state, move |store| store.update(&id, input)).await;
    state.workspace_events.inbox_changed();
    result.map(Json)
}
async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Revision>,
) -> Result<StatusCode, HttpError> {
    blocking(&state, move |store| {
        store.delete(&id, input.expected_revision)
    })
    .await?;
    state.workspace_events.inbox_changed();
    Ok(StatusCode::NO_CONTENT)
}
async fn reorder(
    State(state): State<AppState>,
    Json(input): Json<Reorder>,
) -> Result<StatusCode, HttpError> {
    blocking(&state, |store| store.reorder(input)).await?;
    state.workspace_events.inbox_changed();
    Ok(StatusCode::NO_CONTENT)
}
async fn labels(
    State(state): State<AppState>,
    Json(input): Json<LabelsUpdate>,
) -> Result<StatusCode, HttpError> {
    blocking(&state, |store| store.labels(input)).await?;
    state.workspace_events.inbox_changed();
    Ok(StatusCode::NO_CONTENT)
}
