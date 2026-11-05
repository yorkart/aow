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
