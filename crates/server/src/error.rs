use aow_filesystem::FsError;
use aow_git_service as git;
use aow_protocol::ApiError;
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_RANGE},
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct HttpError {
    pub(crate) status: StatusCode,
    pub(crate) body: ApiError,
    pub(crate) extra_headers: Box<HeaderMap>,
}

impl HttpError {
    pub(crate) fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            status,
            body: ApiError {
                code: code.into(),
                message: message.into(),
                path,
            },
            extra_headers: Box::new(HeaderMap::new()),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            message,
            None,
        )
    }

    pub(crate) fn range_not_satisfiable(size: u64) -> Self {
        let mut error = Self::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "range_not_satisfiable",
            aow_i18n::text("errors.requested_byte_range_is_not_satisfiable"),
            None,
        );
        error.extra_headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{size}")).unwrap(),
        );
        error
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        response.headers_mut().extend(*self.extra_headers);
        response
    }
}

impl From<std::io::Error> for HttpError {
    fn from(error: std::io::Error) -> Self {
        let status = match error.kind() {
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, "io_error", error.to_string(), None)
    }
}

impl From<FsError> for HttpError {
    fn from(error: FsError) -> Self {
        match error {
            FsError::PathNotAbsolute(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "path_not_absolute",
                aow_i18n::text("errors.path_must_be_absolute_2"),
                Some(path),
            ),
            FsError::NotDirectory(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_directory",
                aow_i18n::text("errors.path_is_not_a_directory_2"),
                Some(path),
            ),
            FsError::NotFile(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_file",
                aow_i18n::text("errors.path_is_not_a_regular_file_2"),
                Some(path),
            ),
            FsError::TooLarge {
                path,
                size,
                maximum,
            } => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "file_too_large",
                format!("file is {size} bytes; maximum is {maximum}"),
                Some(path),
            ),
            FsError::NotUtf8(path) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "not_utf8",
                aow_i18n::text("errors.file_is_not_utf"),
                Some(path),
            ),
            FsError::Binary(path) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "binary_file",
                aow_i18n::text("errors.file_contains_nul_bytes_2"),
                Some(path),
            ),
            FsError::VersionConflict {
                path,
                expected,
                current,
            } => Self::new(
                StatusCode::PRECONDITION_FAILED,
                "version_conflict",
                format!("expected {expected}, current {current}"),
                Some(path),
            ),
            FsError::InvalidFileName(name) => Self::new(
                StatusCode::BAD_REQUEST,
                "invalid_file_name",
                aow_i18n::text("errors.name_must_be_a_single_non_empty_file_name"),
                Some(name),
            ),
            FsError::AlreadyExists(path) => Self::new(
                StatusCode::CONFLICT,
                "destination_exists",
                aow_i18n::text("errors.a_file_or_directory_with_that_name_already_exists"),
                Some(path),
            ),
            FsError::Stream(message) => Self::new(
                StatusCode::BAD_REQUEST,
                "request_stream_failed",
                message,
                None,
            ),
            FsError::Io(error) => error.into(),
        }
    }
}

impl From<git::GitError> for HttpError {
    fn from(error: git::GitError) -> Self {
        match error {
            git::GitError::PathNotAbsolute(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "path_not_absolute",
                aow_i18n::text("errors.repository_path_must_be_absolute"),
                Some(path),
            ),
            git::GitError::NotRepository(path) => Self::new(
                StatusCode::NOT_FOUND,
                "git_repository_not_found",
                aow_i18n::text("errors.the_terminal_s_current_directory_is_not_inside_a"),
                Some(path),
            ),
            git::GitError::Timeout => Self::new(
                StatusCode::GATEWAY_TIMEOUT,
                "git_timeout",
                error.to_string(),
                None,
            ),
            git::GitError::Command(_) => Self::new(
                StatusCode::BAD_REQUEST,
                "git_command_failed",
                error.to_string(),
                None,
            ),
            git::GitError::InvalidUtf8 => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "git_invalid_utf8",
                error.to_string(),
                None,
            ),
            git::GitError::Io(error) => error.into(),
            git::GitError::WalkDir(error) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "repository_discovery_failed",
                error.to_string(),
                None,
            ),
        }
    }
}
