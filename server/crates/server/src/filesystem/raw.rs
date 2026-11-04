use super::paths::decode_absolute;
use crate::{AppState, HttpError};
use axum::{
    extract::{Path as AxumPath, State},
    http::HeaderMap,
    response::Response,
};
use std::path::PathBuf;
pub(crate) async fn raw_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let path = decode_absolute(&path)?;
    raw_response(&state, path, headers, false).await
}

pub(super) async fn raw_response(
    state: &AppState,
    path: PathBuf,
    headers: HeaderMap,
    attachment: bool,
) -> Result<Response, HttpError> {
    super::node::download(state, &path, &headers, attachment).await
}
