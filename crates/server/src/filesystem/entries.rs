use std::path::{Component, Path, PathBuf};

use aow_filesystem::{FsError, rename_entry};
use aow_protocol::RenameResult;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{AppState, HttpError};

#[derive(Debug, Deserialize)]
pub(crate) struct CreateFsEntryRequest {
    parent: String,
    name: String,
    kind: FsEntryKind,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FsEntryKind {
    File,
    Directory,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameFsEntryRequest {
    path: String,
    name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FsEntryPathQuery {
    path: String,
}

pub(crate) async fn create_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<CreateFsEntryRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let name = request.name.trim().to_owned();
    validate_fs_entry_name(&name)?;
    let parent = PathBuf::from(&request.parent);
    validate_fs_entry_path(&parent)?;
    if !tokio::fs::metadata(&parent).await?.is_dir() {
        return Err(FsError::NotDirectory(parent.to_string_lossy().into_owned()).into());
    }
    let path = parent.join(&name);
    let result = match request.kind {
        FsEntryKind::File => tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .map(|_| ("file", StatusCode::CREATED)),
        FsEntryKind::Directory => tokio::fs::create_dir(&path)
            .await
            .map(|_| ("directory", StatusCode::CREATED)),
    };
    let (kind, status) = result.map_err(fs_entry_io_error)?;
    Ok((
        status,
        Json(serde_json::json!({
            "path": path.to_string_lossy(),
            "name": name,
            "kind": kind,
        })),
    ))
}

pub(crate) async fn rename_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<RenameFsEntryRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = PathBuf::from(&request.path);
    validate_fs_entry_path(&path)?;
    let destination = rename_entry(path, &request.name).await?;
    Ok(Json(RenameResult {
        path: destination.to_string_lossy().into_owned(),
        name: request.name.trim().to_owned(),
    }))
}

pub(crate) async fn delete_fs_entry(
    State(state): State<AppState>,
    Query(query): Query<FsEntryPathQuery>,
) -> Result<StatusCode, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = PathBuf::from(&query.path);
    validate_fs_entry_path(&path)?;
    if path.parent().is_none() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "root_delete_forbidden",
            "filesystem root cannot be deleted",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        tokio::fs::remove_file(&path).await?;
    } else if metadata.is_dir() {
        tokio::fs::remove_dir_all(&path).await?;
    } else {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_entry",
            "only files, directories, and symbolic links can be deleted",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn validate_fs_entry_name(name: &str) -> Result<(), HttpError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.len() > 255
        || trimmed.contains(['/', '\\', '\0'])
        || Path::new(trimmed)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(trimmed)
    {
        return Err(FsError::InvalidFileName(name.to_owned()).into());
    }
    Ok(())
}

fn validate_fs_entry_path(path: &Path) -> Result<(), HttpError> {
    if !path.is_absolute() {
        return Err(FsError::PathNotAbsolute(path.to_string_lossy().into_owned()).into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_path",
            "path must not contain . or .. components",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    Ok(())
}

fn fs_entry_io_error(error: std::io::Error) -> HttpError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        HttpError::new(
            StatusCode::CONFLICT,
            "destination_exists",
            error.to_string(),
            None,
        )
    } else {
        error.into()
    }
}
