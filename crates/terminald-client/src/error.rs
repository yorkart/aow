use http::StatusCode;
use thiserror::Error;

use aow_protocol::ApiError;
use tokio_tungstenite::tungstenite;

#[derive(Debug, Error)]
pub enum TerminaldClientError {
    #[error("terminald I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminald HTTP transport error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("failed to construct terminald HTTP request: {0}")]
    Http(#[from] http::Error),
    #[error("invalid terminald JSON response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("terminald WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("terminald returned HTTP {status}: {message}")]
    HttpStatus {
        status: StatusCode,
        message: String,
        error: Option<ApiError>,
    },
}
pub(super) fn status_error(status: StatusCode, body: &[u8]) -> TerminaldClientError {
    let error = serde_json::from_slice::<ApiError>(body).ok();
    let message = error
        .as_ref()
        .map(|error| error.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    TerminaldClientError::HttpStatus {
        status,
        message,
        error,
    }
}
