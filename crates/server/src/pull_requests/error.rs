use axum::http::StatusCode;

use crate::HttpError;

pub(super) type Result<T> = std::result::Result<T, PullRequestError>;
#[derive(Debug, thiserror::Error)]
pub enum PullRequestError {
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
impl From<PullRequestError> for HttpError {
    fn from(error: PullRequestError) -> Self {
        let (status, code) = match &error {
            PullRequestError::Invalid(_) => (StatusCode::BAD_REQUEST, "review_provider_invalid"),
            PullRequestError::Conflict => (StatusCode::CONFLICT, "review_provider_conflict"),
            PullRequestError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "review_provider_unavailable",
            ),
            PullRequestError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "review_provider_timeout"),
            PullRequestError::InvalidJson(_) => {
                (StatusCode::BAD_GATEWAY, "review_provider_invalid_response")
            }
            _ => (StatusCode::BAD_GATEWAY, "review_provider_failed"),
        };
        Self::new(status, code, error.to_string(), None)
    }
}
