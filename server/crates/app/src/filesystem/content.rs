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
) -> Result<Json<aow_host_contract::workspace::DirectoryListing>, HttpError> {
    Ok(Json(
        super::node::listing(&state, std::path::Path::new("/")).await?,
    ))
}

pub(crate) async fn list_home(
    State(state): State<AppState>,
) -> Result<Json<aow_host_contract::workspace::DirectoryListing>, HttpError> {
    let home = super::node::home(&state).await?;
    Ok(Json(super::node::listing(&state, &home).await?))
}

pub(crate) async fn list_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Json<aow_host_contract::workspace::DirectoryListing>, HttpError> {
    Ok(Json(
        super::node::listing(&state, &decode_absolute(&path)?).await?,
    ))
}

pub(crate) async fn read_text(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<impl IntoResponse, HttpError> {
    let path = decode_absolute(&path)?;
    let text: aow_host_contract::workspace::TextFile =
        super::node::host(&state)?.workspace_read(&path).await?;
    let etag = HeaderValue::from_str(&format!("\"{}\"", text.version))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, etag);
    Ok((headers, Json(text)))
}
