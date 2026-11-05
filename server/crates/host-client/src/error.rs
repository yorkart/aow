use aow_host_contract::error::ApiError;
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("Host response exceeds the 64 MiB transport limit")]
    ResponseTooLarge,
    #[error("Host HTTP transport failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Invalid Host request: {0}")]
    Request(#[from] http::Error),
    #[error("Host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid Host JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Host WebSocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Host returned HTTP {status}: {message}")]
    HttpStatus {
        status: http::StatusCode,
        message: String,
        error: Option<ApiError>,
    },
}
pub fn is_unavailable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<HostError>()
            .is_some_and(|error| match error {
                HostError::Http(e) => e.is_connect() || e.is_timeout(),
                HostError::Io(_) | HostError::WebSocket(_) => true,
                HostError::HttpStatus { status, .. } => status.is_server_error(),
                _ => false,
            })
    })
}
