use aow_runtime_contract::error::ApiError;
use aow_terminal_core::TerminalError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(crate) struct ServiceError(#[from] TerminalError);

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, code, path) = match &self.0 {
            TerminalError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request", None),
            TerminalError::NotFound(id) => {
                (StatusCode::NOT_FOUND, "runtime_not_found", Some(id.clone()))
            }
            TerminalError::Conflict(id) => {
                (StatusCode::CONFLICT, "runtime_conflict", Some(id.clone()))
            }
            TerminalError::ShuttingDown => (StatusCode::SERVICE_UNAVAILABLE, "shutting_down", None),
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
