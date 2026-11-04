use aow_protocol::RenameResult;
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

#[derive(Debug, Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FsEntryKind {
    File,
    Directory,
}

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
    let _filesystem = state.aow.filesystem_access().await;
    let value: serde_json::Value = super::node::runtime(&state)?
        .workspace_operation(
            "create",
            serde_json::to_value(&request)
                .map_err(|error| HttpError::internal(error.to_string()))?,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(value)))
}

pub(crate) async fn rename_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<RenameFsEntryRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    super::node::runtime(&state)?
        .workspace_operation(
            "rename",
            serde_json::json!({"path":request.path,"name":request.name}),
        )
        .await
        .map(Json)
        .map_err(Into::into)
}

pub(crate) async fn delete_fs_entry(
    State(state): State<AppState>,
    Query(query): Query<FsEntryPathQuery>,
) -> Result<StatusCode, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let _: serde_json::Value = super::node::runtime(&state)?
        .workspace_operation("delete", serde_json::json!({"path":query.path}))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
