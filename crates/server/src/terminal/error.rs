use axum::http::StatusCode;
use thiserror::Error;

use super::MAX_CLIPBOARD_IMAGE_BYTES;
use crate::HttpError;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("{translated}", translated = aow_i18n::message("errors.terminal_tab_not_found", &[("p0", .0.to_string())]))]
    TabNotFound(String),
    #[error("{translated}", translated = aow_i18n::message("errors.terminal_pane_not_found", &[("p0", .0.to_string())]))]
    PaneNotFound(String),
    #[error("{translated}", translated = aow_i18n::message("errors.invalid_terminal_request", &[("p0", .0.to_string())]))]
    Invalid(String),
    #[error("{translated}", translated = aow_i18n::message("errors.terminal_state_conflict", &[("p0", .0.to_string())]))]
    Conflict(String),
    #[error("{translated}", translated = aow_i18n::message("errors.websocket_origin_is_not_allowed", &[("p0", .0.to_string())]))]
    ForbiddenOrigin(String),
    #[error("{translated}", translated = aow_i18n::message("errors.clipboard_image_exceeds_the_byte_limit", &[("p0", MAX_CLIPBOARD_IMAGE_BYTES.to_string())]))]
    ClipboardImageTooLarge,
    #[error("{translated}", translated = aow_i18n::text("errors.clipboard_image_must_be_png_jpeg_gif_or_webp"))]
    UnsupportedClipboardImage,
    #[error("{translated}", translated = aow_i18n::text("errors.clipboard_image_storage_quota_is_exhausted"))]
    ClipboardQuotaExceeded,
    #[error("{translated}", translated = aow_i18n::message("errors.clipboard_image_storage_is_unavailable", &[("p0", .0.to_string())]))]
    ClipboardStorage(String),
    #[error("{translated}", translated = aow_i18n::message("errors.terminald_is_unavailable", &[("p0", .0.to_string())]))]
    DaemonUnavailable(String),
    #[error("{translated}", translated = aow_i18n::message("errors.terminald_request_failed", &[("p0", .0.to_string())]))]
    Daemon(String),
    #[error("{translated}", translated = aow_i18n::message("errors.failed_to_start_terminal", &[("p0", .0.to_string())]))]
    RuntimeCreate(String),
    #[error("{translated}", translated = aow_i18n::text("errors.terminal_state_lock_is_poisoned"))]
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
