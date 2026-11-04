use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("{translated}", translated = aow_i18n::message("errors.path_must_be_absolute", &[("p0", .0.to_string())]))]
    PathNotAbsolute(String),
    #[error("{translated}", translated = aow_i18n::message("errors.path_is_not_a_directory", &[("p0", .0.to_string())]))]
    NotDirectory(String),
    #[error("{translated}", translated = aow_i18n::message("errors.path_is_not_a_regular_file", &[("p0", .0.to_string())]))]
    NotFile(String),
    #[error("{translated}", translated = aow_i18n::message("errors.file_is_too_large_to_edit_bytes_maximum", &[("p0", .size.to_string()), ("p1", .maximum.to_string()), ("p2", .path.to_string())]))]
    TooLarge {
        path: String,
        size: u64,
        maximum: u64,
    },
    #[error("{translated}", translated = aow_i18n::message("errors.file_is_not_utf_text", &[("p0", .0.to_string())]))]
    NotUtf8(String),
    #[error("{translated}", translated = aow_i18n::message("errors.file_contains_nul_bytes", &[("p0", .0.to_string())]))]
    Binary(String),
    #[error("{translated}", translated = aow_i18n::message("errors.file_changed_on_disk_expected_current", &[("p0", .expected.to_string()), ("p1", .current.to_string()), ("p2", .path.to_string())]))]
    VersionConflict {
        path: String,
        expected: String,
        current: String,
    },
    #[error("{translated}", translated = aow_i18n::message("errors.invalid_file_name", &[("p0", .0.to_string())]))]
    InvalidFileName(String),
    #[error("{translated}", translated = aow_i18n::message("errors.destination_already_exists", &[("p0", .0.to_string())]))]
    AlreadyExists(String),
    #[error("{translated}", translated = aow_i18n::message("errors.streaming_request_failed", &[("p0", .0.to_string())]))]
    Stream(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
