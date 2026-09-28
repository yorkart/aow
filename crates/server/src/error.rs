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
            "requested byte range is not satisfiable",
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
                "path must be absolute",
                Some(path),
            ),
            FsError::NotDirectory(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_directory",
                "path is not a directory",
                Some(path),
            ),
            FsError::NotFile(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_file",
                "path is not a regular file",
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
                "file is not UTF-8",
                Some(path),
            ),
            FsError::Binary(path) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "binary_file",
                "file contains NUL bytes",
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
                "name must be a single non-empty file name",
                Some(name),
            ),
            FsError::AlreadyExists(path) => Self::new(
                StatusCode::CONFLICT,
                "destination_exists",
                "a file or directory with that name already exists",
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
                "repository path must be absolute",
                Some(path),
            ),
            git::GitError::NotRepository(path) => Self::new(
                StatusCode::NOT_FOUND,
                "git_repository_not_found",
                "the terminal's current directory is not inside a Git repository",
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
