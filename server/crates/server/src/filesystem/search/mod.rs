mod requests;
#[cfg(test)]
mod tests;

use aow_filesystem::SearchError;
#[cfg(test)]
use aow_filesystem::search_files;
use aow_protocol::FileSearchRequest;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{AppState, HttpError};
pub(crate) use requests::SearchRequests;

#[derive(Deserialize, serde::Serialize)]
pub(crate) struct Request {
    request_id: String,
    #[serde(flatten)]
    search: FileSearchRequest,
}

pub(crate) async fn search(
    State(state): State<AppState>,
    Json(request): Json<Request>,
) -> Result<Json<aow_protocol::FileSearchResult>, HttpError> {
    let guard = state.file_search.begin(request.request_id.clone())?;
    let _permit = tokio::select! {
        _ = guard.cancel.cancelled() => return Err(search_error(SearchError::Cancelled)),
        permit = state.file_search.capacity.acquire() => permit.map_err(|error| HttpError::internal(error.to_string()))?,
    };
    if let Some(runtime) = state.acp.runtime() {
        let mut remote = RemoteSearch {
            client: runtime.clone(),
            id: request.request_id.clone(),
            finished: false,
        };
        let result = tokio::select! {
            _=guard.cancel.cancelled()=>return Err(search_error(SearchError::Cancelled)),
            result=runtime.post_json("/v1/workspace/search",&request)=>result,
        };
        remote.finished = true;
        return result.map(Json).map_err(Into::into);
    }
    #[cfg(not(test))]
    {
        Err(super::node::runtime(&state).unwrap_err())
    }
    #[cfg(test)]
    {
        Ok(Json(
            search_files(request.search, guard.cancel.clone())
                .await
                .map_err(search_error)?,
        ))
    }
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.file_search.cancel(request_id.clone())?;
    if let Some(runtime) = state.acp.runtime() {
        runtime
            .delete(&format!("/v1/workspace/search/{request_id}"))
            .await?;
    }
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

struct RemoteSearch {
    client: aow_runtime_client::RuntimeClient,
    id: String,
    finished: bool,
}
impl Drop for RemoteSearch {
    fn drop(&mut self) {
        if !self.finished
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let client = self.client.clone();
            let id = self.id.clone();
            handle.spawn(async move {
                let _ = client.delete(&format!("/v1/workspace/search/{id}")).await;
            });
        }
    }
}
