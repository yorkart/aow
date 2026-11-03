//! Human notification state is independent of machine wakeup delivery.
use crate::{AppState, HttpError};
use aow_db::tasks::InboxPage;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde::Deserialize;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/inbox", get(list))
        .route("/api/inbox/{id}", patch(update))
        .route("/api/inbox/{id}/retry", post(retry))
}

#[derive(Deserialize, Default)]
struct Filter {
    project_id: Option<String>,
    #[serde(default)]
    unread: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    offset: u32,
    limit: Option<u32>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<Filter>,
) -> Result<Json<InboxPage>, HttpError> {
    state
        .aow
        .task_db()
        .inbox(
            filter.project_id.as_deref(),
            filter.unread,
            filter.archived,
            filter.offset,
            filter.limit.unwrap_or(30).clamp(1, 100),
        )
        .map(Json)
        .map_err(storage)
}

#[derive(Deserialize)]
struct Update {
    read: Option<bool>,
    archived: Option<bool>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<Update>,
) -> Result<axum::http::StatusCode, HttpError> {
    state
        .aow
        .task_db()
        .update_inbox(&id, update.read, update.archived)
        .map_err(storage)?;
    state.workspace_events.inbox_changed();
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, HttpError> {
    state
        .aow
        .task_db()
        .retry_delivery(&id, &format!("wakeup-{}", uuid::Uuid::new_v4()))
        .map_err(|error| crate::instances::invalid(error.to_string()))?;
    state.workspace_events.inbox_changed();
    state.collaboration.start(&state);
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn storage(error: anyhow::Error) -> HttpError {
    HttpError::internal(error.to_string())
}
