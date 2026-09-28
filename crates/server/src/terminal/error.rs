use axum::http::StatusCode;
use thiserror::Error;

use super::MAX_CLIPBOARD_IMAGE_BYTES;
use crate::HttpError;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal tab not found: {0}")]
    TabNotFound(String),
    #[error("terminal pane not found: {0}")]
    PaneNotFound(String),
    #[error("invalid terminal request: {0}")]
    Invalid(String),
    #[error("terminal state conflict: {0}")]
    Conflict(String),
    #[error("WebSocket origin is not allowed: {0}")]
    ForbiddenOrigin(String),
    #[error("clipboard image exceeds the {MAX_CLIPBOARD_IMAGE_BYTES}-byte limit")]
    ClipboardImageTooLarge,
    #[error("clipboard image must be PNG, JPEG, GIF, or WebP")]
    UnsupportedClipboardImage,
    #[error("clipboard image storage quota is exhausted")]
    ClipboardQuotaExceeded,
    #[error("clipboard image storage is unavailable: {0}")]
    ClipboardStorage(String),
    #[error("terminald is unavailable: {0}")]
    DaemonUnavailable(String),
    #[error("terminald request failed: {0}")]
    Daemon(String),
    #[error("failed to start terminal: {0}")]
    RuntimeCreate(String),
    #[error("terminal state lock is poisoned")]
    Poisoned,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub(crate) fn terminal_http_error(error: TerminalError) -> HttpError {
    match error {
        TerminalError::TabNotFound(_) | TerminalError::PaneNotFound(_) => HttpError::new(
            StatusCode::NOT_FOUND,
            "terminal_not_found",
            error.to_string(),
            None,
        ),
        TerminalError::Invalid(_) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_terminal_request",
            error.to_string(),
            None,
        ),
        TerminalError::Conflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "terminal_conflict",
            error.to_string(),
            None,
        ),
        TerminalError::ForbiddenOrigin(_) => HttpError::new(
            StatusCode::FORBIDDEN,
            "forbidden_websocket_origin",
            error.to_string(),
            None,
        ),
        TerminalError::ClipboardImageTooLarge => HttpError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "clipboard_image_too_large",
            error.to_string(),
            None,
        ),
        TerminalError::UnsupportedClipboardImage => HttpError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_clipboard_image",
            error.to_string(),
            None,
        ),
        TerminalError::ClipboardQuotaExceeded => HttpError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "clipboard_image_quota_exceeded",
            error.to_string(),
            None,
        ),
        TerminalError::DaemonUnavailable(_) => HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminald_unavailable",
            error.to_string(),
            None,
        ),
        TerminalError::RuntimeCreate(_) => HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "terminal_spawn_failed",
            error.to_string(),
            None,
        ),
        TerminalError::Daemon(_)
        | TerminalError::Poisoned
        | TerminalError::ClipboardStorage(_)
        | TerminalError::Io(_)
        | TerminalError::Json(_) => HttpError::internal(error.to_string()),
    }
}
