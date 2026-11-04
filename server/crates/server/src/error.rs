use aow_git_service as git;
use aow_protocol::ApiError;
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
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

impl From<aow_runtime_client::RuntimeError> for HttpError {
    fn from(error: aow_runtime_client::RuntimeError) -> Self {
        match error {
            aow_runtime_client::RuntimeError::HttpStatus {
                status,
                error: Some(body),
                ..
            } => Self {
                status,
                body,
                extra_headers: Box::default(),
            },
            aow_runtime_client::RuntimeError::HttpStatus {
                status, message, ..
            } => Self::new(status, "runtime_error", message, None),
            _ => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "runtime_unavailable",
                "Runtime service is unavailable",
                None,
            ),
        }
    }
}
