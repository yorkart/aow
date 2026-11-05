use aow_runtime_contract::workspace::RenameResult;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{AppState, HttpError};

#[derive(Debug, Deserialize, serde::Serialize)]
pub(crate) struct CreateFsEntryRequest {
    parent: String,
    name: String,
    kind: FsEntryKind,
}

use aow_runtime_contract::workspace::EntryKind as FsEntryKind;

#[derive(Debug, Deserialize, serde::Serialize)]
pub(crate) struct RenameFsEntryRequest {
    path: String,
    name: String,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub(crate) struct FsEntryPathQuery {
    path: String,
}

pub(crate) async fn create_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<CreateFsEntryRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let _filesystem = state.projects.filesystem_access().await;
    let value = super::node::runtime(&state)?
        .workspace_create(
            std::path::Path::new(&request.parent),
            &request.name,
            request.kind,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(value)))
}

pub(crate) async fn rename_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<RenameFsEntryRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.projects.filesystem_access().await;
    super::node::runtime(&state)?
        .workspace_rename(std::path::Path::new(&request.path), &request.name, false)
        .await
        .map(Json)
        .map_err(Into::into)
}

pub(crate) async fn delete_fs_entry(
    State(state): State<AppState>,
    Query(query): Query<FsEntryPathQuery>,
) -> Result<StatusCode, HttpError> {
    let _filesystem = state.projects.filesystem_access().await;
    let () = super::node::runtime(&state)?
        .workspace_delete(std::path::Path::new(&query.path))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
