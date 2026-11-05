mod requests;
#[cfg(test)]
mod tests;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{AppState, HttpError};
pub(crate) use requests::SearchRequests;

use aow_host_contract::workspace::SearchRequest as Request;

pub(crate) async fn search(
    State(state): State<AppState>,
    Json(request): Json<Request>,
) -> Result<Json<aow_host_contract::workspace::FileSearchResult>, HttpError> {
    let guard = state.file_search.begin(request.request_id.clone())?;
    let _permit = tokio::select! {
        _ = guard.cancel.cancelled() => return Err(cancelled()),
        permit = state.file_search.capacity.acquire() => permit.map_err(|error| HttpError::internal(error.to_string()))?,
    };
    let runtime = super::node::host(&state)?;
    let mut remote = RemoteSearch {
        client: runtime.clone(),
        id: request.request_id.clone(),
        finished: false,
    };
    let result = tokio::select! {
        _=guard.cancel.cancelled()=>return Err(cancelled()),
        result=runtime.search_workspace(&request)=>result,
    };
    remote.finished = true;
    result.map(Json).map_err(Into::into)
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.file_search.cancel(request_id.clone())?;
    super::node::host(&state)?
        .cancel_workspace_search(&request_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn cancelled() -> HttpError {
    HttpError::new(
        StatusCode::CONFLICT,
        "search_cancelled",
        aow_i18n::text("filesystem.search_canceled"),
        None,
    )
}

struct RemoteSearch {
    client: aow_host_client::HostClient,
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
                let _ = client.cancel_workspace_search(&id).await;
            });
        }
    }
}
