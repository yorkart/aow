use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::HttpError;

#[derive(Debug, Error)]
pub(crate) enum ServiceError {
    #[error("{translated}", translated = aow_i18n::message("errors.invalid_aow_request", &[("p0", .0.to_string())]))]
    Invalid(String),
    #[error("{translated}", translated = aow_i18n::message("errors.project_not_found", &[("p0", .0.to_string())]))]
    ProjectNotFound(String),
    #[error("{translated}", translated = aow_i18n::message("errors.worktree_has_uncommitted_change_s_force_removal_is_required", &[("p0", .0.to_string())]))]
    DirtyWorktree(usize),
    #[error("{0}")]
    RemovalConflict(String),
    #[error("{message}", message = aow_i18n::message("server.agent_id_already_exists", &[("p0", .0.to_string())]))]
    AgentIdConflict(String),
    #[error("{translated}", translated = aow_i18n::message("errors.agent_not_found_or_unavailable", &[("p0", .0.to_string())]))]
    AgentNotFound(String),
    #[error("{translated}", translated = aow_i18n::message("errors.git_command_failed", &[("p0", .0.to_string())]))]
    Git(String),
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    CreationConflict(String),
    #[error("{translated}", translated = aow_i18n::message("errors.configuration_storage", &[("p0", format!("{:#}", .0))]))]
    Configuration(#[source] anyhow::Error),
    #[error("{translated}", translated = aow_i18n::text("errors.aow_state_lock_is_poisoned"))]
    Poisoned,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub(crate) fn service_http_error(error: ServiceError) -> HttpError {
    match error {
        ServiceError::AgentIdConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "agent_id_exists",
            error.to_string(),
            None,
        ),
        ServiceError::CreationConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "worktree_creating",
            error.to_string(),
            None,
        ),
        ServiceError::RemovalConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "worktree_removing",
            error.to_string(),
            None,
        ),
        ServiceError::DirtyWorktree(_) => HttpError::new(
            StatusCode::CONFLICT,
            "dirty_worktree",
            error.to_string(),
            None,
        ),
        ServiceError::Invalid(_) | ServiceError::Git(_) | ServiceError::Timeout(_) => {
            HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_aow_request",
                error.to_string(),
                None,
            )
        }
        ServiceError::ProjectNotFound(_) | ServiceError::AgentNotFound(_) => HttpError::new(
            StatusCode::NOT_FOUND,
            "aow_item_not_found",
            error.to_string(),
            None,
        ),
        ServiceError::Io(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            HttpError::new(
                StatusCode::CONFLICT,
                "notes_entry_exists",
                error.to_string(),
                None,
            )
        }
        ServiceError::Io(error) => error.into(),
        ServiceError::Poisoned | ServiceError::Json(_) | ServiceError::Configuration(_) => {
            HttpError::internal(error.to_string())
        }
    }
}

pub(crate) fn service_response(error: ServiceError) -> Response {
    service_http_error(error).into_response()
}
