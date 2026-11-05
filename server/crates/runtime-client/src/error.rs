use aow_runtime_contract::error::ApiError;
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Runtime response exceeds the 64 MiB transport limit")]
    ResponseTooLarge,
    #[error("Runtime HTTP transport failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Invalid runtime request: {0}")]
    Request(#[from] http::Error),
    #[error("Runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid runtime JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Runtime WebSocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Runtime returned HTTP {status}: {message}")]
    HttpStatus {
        status: http::StatusCode,
        message: String,
        error: Option<ApiError>,
    },
}
pub fn is_unavailable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<RuntimeError>()
            .is_some_and(|error| match error {
                RuntimeError::Http(e) => e.is_connect() || e.is_timeout(),
                RuntimeError::Io(_) | RuntimeError::WebSocket(_) => true,
                RuntimeError::HttpStatus { status, .. } => status.is_server_error(),
                _ => false,
            })
    })
}
