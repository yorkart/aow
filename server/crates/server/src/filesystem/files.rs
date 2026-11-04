#[cfg(test)]
use aow_filesystem::{rename_file, write_atomic_stream, write_unique_stream};
use aow_protocol::RenameResult;
use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
};
#[cfg(test)]
use axum::{
    http::{
        HeaderValue, StatusCode,
        header::{ETAG, IF_MATCH},
    },
    response::IntoResponse,
};
#[cfg(test)]
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
) -> Result<axum::response::Response, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = decode_absolute(&path)?;
    if state.acp.runtime().is_some() {
        return super::node::upload(&state, &path, &headers, body, query.keep_both).await;
    }
    #[cfg(not(test))]
    {
        let _ = (path, headers, body, query);
        Err(super::node::runtime(&state).unwrap_err())
    }
    #[cfg(test)]
    {
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
        Ok((status, response_headers, Json(result)).into_response())
    }
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
    if let Some(runtime) = state.acp.runtime() {
        return runtime.workspace_operation("rename",serde_json::json!({"path":decode_absolute(&path)?,"name":request.name,"file_only":true})).await.map(Json).map_err(Into::into);
    }
    #[cfg(not(test))]
    {
        let _ = (path, request);
        Err(super::node::runtime(&state).unwrap_err())
    }
    #[cfg(test)]
    {
        let destination = rename_file(decode_absolute(&path)?, &request.name).await?;
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                HttpError::internal(aow_i18n::text(
                    "validation.renamed_file_does_not_have_a_utf_8_name",
                ))
            })?
            .to_owned();
        Ok(Json(RenameResult {
            path: destination.to_string_lossy().into_owned(),
            name,
        }))
    }
}
