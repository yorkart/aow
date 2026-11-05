use aow_api_contract::ApiError;
use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

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

impl From<aow_host_client::HostError> for HttpError {
    fn from(error: aow_host_client::HostError) -> Self {
        match error {
            aow_host_client::HostError::HttpStatus {
                status,
                error: Some(body),
                ..
            } => Self {
                status,
                body,
                extra_headers: Box::default(),
            },
            aow_host_client::HostError::HttpStatus {
                status, message, ..
            } => Self::new(status, "runtime_error", message, None),
            _ => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "host_unavailable",
                "Host service is unavailable",
                None,
            ),
        }
    }
}

mod service;
pub(crate) use service::ServiceError;
pub(crate) use service::service_http_error;
pub(crate) use service::service_response;
