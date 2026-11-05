use aow_runtime_contract::workspace::RenameResult;
use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;

use super::paths::decode_absolute;
use crate::{AppState, HttpError};

#[derive(Default, Deserialize)]
pub(crate) struct WriteFileQuery {
    #[serde(default)]
    keep_both: bool,
}

pub(crate) async fn write_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<WriteFileQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<axum::response::Response, HttpError> {
    let _filesystem = state.projects.filesystem_access().await;
    let path = decode_absolute(&path)?;
    super::node::upload(&state, &path, &headers, body, query.keep_both).await
}

#[derive(Deserialize)]
pub(crate) struct RenameFileRequest {
    name: String,
}

pub(crate) async fn rename_file_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    Json(request): Json<RenameFileRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.projects.filesystem_access().await;
    super::node::runtime(&state)?
        .workspace_rename(&decode_absolute(&path)?, &request.name, true)
        .await
        .map(Json)
        .map_err(Into::into)
}
