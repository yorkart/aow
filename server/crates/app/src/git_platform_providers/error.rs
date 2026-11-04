use axum::http::StatusCode;

use crate::HttpError;

pub(super) type Result<T> = std::result::Result<T, GitPlatformError>;
#[derive(Debug, thiserror::Error)]
pub enum GitPlatformError {
    #[error("{0}")]
    Invalid(String),
    #[error("{message}", message = aow_i18n::text("server.configuration_updated_reload_before_saving"))]
    Conflict,
    #[error("{0}")]
    Unavailable(String),
    #[error("{message}", message = aow_i18n::text("server.provider_script_execution_timed_out"))]
    Timeout,
    #[error("{0}")]
    Command(String),
    #[error("{message}", message = aow_i18n::message("server.provider_response_violates_the_protocol", &[("p0", .0.to_string())]))]
    InvalidJson(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
impl From<GitPlatformError> for HttpError {
    fn from(error: GitPlatformError) -> Self {
        let (status, code) = match &error {
            GitPlatformError::Invalid(_) => {
                (StatusCode::BAD_REQUEST, "git_platform_provider_invalid")
            }
            GitPlatformError::Conflict => (StatusCode::CONFLICT, "git_platform_provider_conflict"),
            GitPlatformError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "git_platform_provider_unavailable",
            ),
            GitPlatformError::Timeout => {
                (StatusCode::GATEWAY_TIMEOUT, "git_platform_provider_timeout")
            }
            GitPlatformError::InvalidJson(_) => (
                StatusCode::BAD_GATEWAY,
                "git_platform_provider_invalid_response",
            ),
            _ => (StatusCode::BAD_GATEWAY, "git_platform_provider_failed"),
        };
        Self::new(status, code, error.to_string(), None)
    }
}
