mod requests;
#[cfg(test)]
mod tests;

use aow_filesystem::{SearchError, search_files};
use aow_protocol::FileSearchRequest;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{AppState, HttpError};
pub(crate) use requests::SearchRequests;

#[derive(Deserialize)]
pub(crate) struct Request {
    request_id: String,
    #[serde(flatten)]
    search: FileSearchRequest,
}

pub(crate) async fn search(
    State(state): State<AppState>,
    Json(request): Json<Request>,
) -> Result<Json<aow_protocol::FileSearchResult>, HttpError> {
    let guard = state.file_search.begin(request.request_id)?;
    let _permit = tokio::select! {
        _ = guard.cancel.cancelled() => return Err(search_error(SearchError::Cancelled)),
        permit = state.file_search.capacity.acquire() => permit.map_err(|error| HttpError::internal(error.to_string()))?,
    };
    Ok(Json(
        search_files(request.search, guard.cancel.clone())
            .await
            .map_err(search_error)?,
    ))
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.file_search.cancel(request_id)?;
    Ok(StatusCode::NO_CONTENT)
}

fn search_error(error: SearchError) -> HttpError {
    let (status, code) = match &error {
        SearchError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "ripgrep_unavailable"),
        SearchError::Cancelled => (StatusCode::CONFLICT, "search_cancelled"),
        SearchError::Invalid(_) | SearchError::Failed(_) => {
            (StatusCode::BAD_REQUEST, "search_failed")
        }
        SearchError::Io(_) => (StatusCode::BAD_REQUEST, "search_io_error"),
    };
    HttpError::new(status, code, error.to_string(), None)
}
