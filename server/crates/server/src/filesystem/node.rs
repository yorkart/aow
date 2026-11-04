//! Server-side forwarding only; file handles and Git processes belong to the node.
use crate::{AppState, HttpError};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use std::path::Path;
pub(super) fn runtime(state: &AppState) -> Result<&aow_runtime_client::RuntimeClient, HttpError> {
    state.acp.runtime().ok_or_else(|| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_unavailable",
            "Workspace runtime is unavailable",
            None,
        )
    })
}
pub(super) fn forwarded(response: reqwest::Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let mut forwarded = Response::new(Body::from_stream(response.bytes_stream()));
    *forwarded.status_mut() = status;
    for name in [
        http::header::CONTENT_TYPE,
        http::header::CONTENT_LENGTH,
        http::header::CONTENT_RANGE,
        http::header::ACCEPT_RANGES,
        http::header::ETAG,
        http::header::CACHE_CONTROL,
        http::header::CONTENT_DISPOSITION,
        http::header::CONTENT_SECURITY_POLICY,
        http::header::X_CONTENT_TYPE_OPTIONS,
    ] {
        if let Some(value) = headers.get(&name) {
            forwarded.headers_mut().insert(name, value.clone());
        }
    }
    forwarded
}
pub(super) async fn download(
    state: &AppState,
    path: &Path,
    headers: &HeaderMap,
    attachment: bool,
) -> Result<Response, HttpError> {
    Ok(forwarded(
        runtime(state)?
            .workspace_transfer(Method::GET, path, false, attachment, headers, None)
            .await?,
    ))
}
pub(super) async fn upload(
    state: &AppState,
    path: &Path,
    headers: &HeaderMap,
    body: Body,
    keep_both: bool,
) -> Result<Response, HttpError> {
    let body = reqwest::Body::wrap_stream(body.into_data_stream());
    Ok(forwarded(
        runtime(state)?
            .workspace_transfer(Method::PUT, path, keep_both, false, headers, Some(body))
            .await?,
    ))
}
pub(super) async fn listing(
    state: &AppState,
    path: &Path,
) -> Result<aow_protocol::DirectoryListing, HttpError> {
    if let Some(runtime) = state.acp.runtime() {
        return Ok(runtime
            .workspace_operation("list", serde_json::json!({"path":path}))
            .await?);
    }
    #[cfg(test)]
    {
        Ok(aow_filesystem::list_directory(path).await?)
    }
    #[cfg(not(test))]
    {
        let _ = path;
        Err(runtime(state).unwrap_err())
    }
}
pub(super) async fn home(state: &AppState) -> Result<std::path::PathBuf, HttpError> {
    if let Some(runtime) = state.acp.runtime() {
        return Ok(runtime.workspace_home().await?);
    }
    #[cfg(test)]
    {
        Ok(super::paths::PROCESS_HOME.clone())
    }
    #[cfg(not(test))]
    {
        Err(runtime(state).unwrap_err())
    }
}
pub(super) async fn is_file(state: &AppState, path: &Path) -> Result<bool, HttpError> {
    if let Some(runtime) = state.acp.runtime() {
        let metadata = runtime.workspace_metadata(path, true).await?;
        if !metadata.exists {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        return Ok(metadata.file);
    }
    #[cfg(test)]
    {
        Ok(tokio::fs::metadata(path).await?.is_file())
    }
    #[cfg(not(test))]
    {
        let _ = path;
        Err(runtime(state).unwrap_err())
    }
}
