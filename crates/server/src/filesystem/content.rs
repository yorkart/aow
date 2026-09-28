use aow_filesystem::{DEFAULT_MAX_TEXT_BYTES, list_directory, read_text_file};
use axum::{
    Json,
    extract::Path as AxumPath,
    http::{HeaderMap, HeaderValue, header::ETAG},
    response::IntoResponse,
};

use super::paths::decode_absolute;
use crate::HttpError;

pub(crate) async fn list_root() -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory("/").await?))
}

pub(crate) async fn list_home() -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory(&*super::paths::PROCESS_HOME).await?))
}

pub(crate) async fn list_path(
    AxumPath(path): AxumPath<String>,
) -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory(decode_absolute(&path)?).await?))
}

pub(crate) async fn read_text(
    AxumPath(path): AxumPath<String>,
) -> Result<impl IntoResponse, HttpError> {
    let text = read_text_file(decode_absolute(&path)?, DEFAULT_MAX_TEXT_BYTES).await?;
    let etag = HeaderValue::from_str(&format!("\"{}\"", text.version))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, etag);
    Ok((headers, Json(text)))
}
