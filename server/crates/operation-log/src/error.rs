use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}", message = aow_i18n::message("operation-log.log_read_write_failed", &[("p0", .0.to_string())]))]
    Io(#[from] io::Error),
    #[error("{message}", message = aow_i18n::message("operation-log.invalid_log_format", &[("p0", .0.to_string())]))]
    Json(#[from] serde_json::Error),
    #[error("{message}", message = aow_i18n::text("operation-log.invalid_log_cursor"))]
    InvalidCursor,
    #[error("{message}", message = aow_i18n::text("operation-log.log_file_expired_or_changed_refresh_logs"))]
    CursorExpired,
    #[error("{message}", message = aow_i18n::text("operation-log.invalid_log_query_parameters"))]
    InvalidOptions,
    #[error("{message}", message = aow_i18n::text("operation-log.log_entry_exceeds_size_limit"))]
    RecordTooLarge,
}
