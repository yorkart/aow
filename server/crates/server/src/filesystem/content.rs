#[cfg(test)]
use aow_filesystem::{DEFAULT_MAX_TEXT_BYTES, read_text_file};
use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, header::ETAG},
    response::IntoResponse,
};

use super::paths::decode_absolute;
use crate::{AppState, HttpError};

pub(crate) async fn list_root(
    State(state): State<AppState>,
) -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(
        super::node::listing(&state, std::path::Path::new("/")).await?,
    ))
}

pub(crate) async fn list_home(
    State(state): State<AppState>,
) -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    let home = super::node::home(&state).await?;
    Ok(Json(super::node::listing(&state, &home).await?))
}

pub(crate) async fn list_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(
        super::node::listing(&state, &decode_absolute(&path)?).await?,
    ))
}

pub(crate) async fn read_text(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<impl IntoResponse, HttpError> {
    let path = decode_absolute(&path)?;
    let text: aow_protocol::TextFile = if let Some(runtime) = state.acp.runtime() {
        runtime
            .workspace_operation("read", serde_json::json!({"path":path}))
            .await?
    } else {
        #[cfg(test)]
        {
            read_text_file(path, DEFAULT_MAX_TEXT_BYTES).await?
        }
        #[cfg(not(test))]
        {
            return Err(super::node::runtime(&state).unwrap_err());
        }
    };
    let etag = HeaderValue::from_str(&format!("\"{}\"", text.version))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, etag);
    Ok((headers, Json(text)))
}
