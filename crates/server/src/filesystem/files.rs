use aow_filesystem::{rename_file, write_atomic_stream, write_unique_stream};
use aow_protocol::RenameResult;
use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_MATCH},
    },
    response::IntoResponse,
};
use futures_util::TryStreamExt;
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
) -> Result<impl IntoResponse, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = decode_absolute(&path)?;
    let expected = headers
        .get(IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('\"'));
    let stream = body.into_data_stream().map_err(|error| error.to_string());
    let result = if query.keep_both {
        write_unique_stream(path, stream).await?
    } else {
        write_atomic_stream(path, expected, stream).await?
    };
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        ETAG,
        HeaderValue::from_str(&format!("\"{}\"", result.version))
            .map_err(|error| HttpError::internal(error.to_string()))?,
    );
    Ok((status, response_headers, Json(result)))
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
    let _filesystem = state.aow.filesystem_access().await;
    let destination = rename_file(decode_absolute(&path)?, &request.name).await?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| HttpError::internal("renamed file does not have a UTF-8 name"))?
        .to_owned();
    Ok(Json(RenameResult {
        path: destination.to_string_lossy().into_owned(),
        name,
    }))
}
