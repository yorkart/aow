use http::StatusCode;
use thiserror::Error;

use aow_api_contract::ApiError;
use tokio_tungstenite::tungstenite;

#[derive(Debug, Error)]
pub enum ServerClientError {
    #[error("Server HTTP transport error: {0}")]
    Remote(#[from] reqwest::Error),
    #[error("local CLI I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("local CLI HTTP transport error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("failed to construct local CLI HTTP request: {0}")]
    Http(#[from] http::Error),
    #[error("invalid Server CLI JSON response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("local CLI WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("Server CLI returned HTTP {status}: {message}")]
    HttpStatus {
        status: StatusCode,
        message: String,
        error: Option<ApiError>,
    },
}
pub(super) fn status_error(status: StatusCode, body: &[u8]) -> ServerClientError {
    let error = serde_json::from_slice::<ApiError>(body).ok();
    let message = error
        .as_ref()
        .map(|error| error.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    ServerClientError::HttpStatus {
        status,
        message,
        error,
    }
}
