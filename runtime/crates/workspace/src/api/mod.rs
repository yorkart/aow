//! Authenticated listener composition is owned by runtime-daemon.
pub(crate) mod error;
mod operations;
mod raw;
mod search;
mod search_requests;
mod watch;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use error::HttpError;
use futures_util::TryStreamExt;
use serde::Deserialize;
#[derive(Clone)]
struct WorkspaceState {
    search: search_requests::SearchRequests,
    stopping: tokio::sync::watch::Receiver<bool>,
    clipboard: std::sync::Arc<crate::clipboard::Clipboard>,
}
pub fn router(
    root: &std::path::Path,
    stopping: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Router> {
    let state = WorkspaceState {
        search: Default::default(),
        clipboard: std::sync::Arc::new(crate::clipboard::Clipboard::open(root)?),
        stopping,
    };
    let clipboard = std::sync::Arc::downgrade(&state.clipboard);
    let mut stopped = state.stopping.clone();
    aow_i18n::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tokio::select! {_=stopped.changed()=>return,_=interval.tick()=>{let Some(clipboard)=clipboard.upgrade()else{return};if let Err(error)=clipboard.clean(){tracing::warn!(%error,"clipboard cleanup failed");}}}
        }
    });
    Ok(Router::new()
        .route("/v1/workspace/operations", post(operations::operation))
        .route(
            "/v1/workspace/raw",
            get(download).put(upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/v1/workspace/search", post(search::search))
        .route(
            "/v1/workspace/search/{id}",
            axum::routing::delete(search::cancel),
        )
        .route("/v1/workspace/watch", get(watch::subscribe))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admission,
        ))
        .route(
            "/v1/workspace/clipboard",
            post(clipboard_upload).layer(DefaultBodyLimit::disable()),
        )
        .with_state(state))
}
#[derive(Deserialize)]
struct FileQuery {
    path: std::path::PathBuf,
    #[serde(default)]
    keep_both: bool,
    #[serde(default)]
    attachment: bool,
}
async fn download(
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<axum::response::Response, HttpError> {
    raw::raw_response(query.path, headers, query.attachment).await
}
async fn upload(
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl axum::response::IntoResponse, HttpError> {
    let expected = headers
        .get(http::header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'));
    let stream = body.into_data_stream().map_err(|error| error.to_string());
    let result = if query.keep_both {
        aow_filesystem::write_unique_stream(query.path, stream).await?
    } else {
        aow_filesystem::write_atomic_stream(query.path, expected, stream).await?
    };
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let headers = [(http::header::ETAG, format!("\"{}\"", result.version))];
    Ok((status, headers, Json(result)))
}

async fn admission(
    State(state): State<WorkspaceState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut stopping = state.stopping.clone();
    if *stopping.borrow() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    tokio::select! {response=next.run(request)=>response,_=stopping.changed()=>StatusCode::SERVICE_UNAVAILABLE.into_response()}
}

async fn clipboard_upload(
    State(state): State<WorkspaceState>,
    body: Body,
) -> Result<Json<serde_json::Value>, HttpError> {
    state.clipboard.upload(body).await
}
