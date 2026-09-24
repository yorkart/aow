use std::path::PathBuf;

use aow_protocol::ApiError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TerminaldError {
    #[error("invalid terminal runtime request: {0}")]
    Invalid(String),
    #[error("terminal runtime not found: {0}")]
    NotFound(String),
    #[error("terminal runtime conflict: {0}")]
    Conflict(String),
    #[error("failed to start terminal runtime: {0}")]
    Pty(String),
    #[error("terminal runtime worker failed: {0}")]
    Worker(String),
    #[error("terminald state lock is poisoned")]
    Poisoned,
    #[error("terminald is shutting down")]
    ShuttingDown,
    #[error("terminald socket is already in use: {0}")]
    SocketInUse(PathBuf),
    #[error("unsafe terminald socket path: {0}")]
    UnsafeSocket(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl IntoResponse for TerminaldError {
    fn into_response(self) -> Response {
        let (status, code, path) = match &self {
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request", None),
            Self::NotFound(id) => (StatusCode::NOT_FOUND, "runtime_not_found", Some(id.clone())),
            Self::Conflict(id) => (StatusCode::CONFLICT, "runtime_conflict", Some(id.clone())),
            Self::ShuttingDown => (StatusCode::SERVICE_UNAVAILABLE, "shutting_down", None),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None),
        };
        let body = ApiError {
            code: code.to_owned(),
            message: self.to_string(),
            path,
        };
        (status, Json(body)).into_response()
    }
}
