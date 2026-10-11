use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::HttpError;

#[derive(Debug, Error)]
pub(crate) enum AowError {
    #[error("invalid AoW request: {0}")]
    Invalid(String),
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("worktree has {0} uncommitted change(s); force removal is required")]
    DirtyWorktree(usize),
    #[error("{0}")]
    RemovalConflict(String),
    #[error("Agent ID 已存在：{0}")]
    AgentIdConflict(String),
    #[error("agent not found or unavailable: {0}")]
    AgentNotFound(String),
    #[error("git command failed: {0}")]
    Git(String),
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    CreationConflict(String),
    #[error("configuration repository: {0:#}")]
    Configuration(#[source] anyhow::Error),
    #[error("AoW state lock is poisoned")]
    Poisoned,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<aow_agents::launch::LaunchError> for AowError {
    fn from(error: aow_agents::launch::LaunchError) -> Self {
        match error {
            aow_agents::launch::LaunchError::Unavailable(id) => Self::AgentNotFound(id),
            aow_agents::launch::LaunchError::Invalid(message) => Self::Invalid(message),
        }
    }
}

pub(crate) fn aow_http_error(error: AowError) -> HttpError {
    match error {
        AowError::AgentIdConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "agent_id_exists",
            error.to_string(),
            None,
        ),
        AowError::CreationConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "worktree_creating",
            error.to_string(),
            None,
        ),
        AowError::RemovalConflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "worktree_removing",
            error.to_string(),
            None,
        ),
        AowError::DirtyWorktree(_) => HttpError::new(
            StatusCode::CONFLICT,
            "dirty_worktree",
            error.to_string(),
            None,
        ),
        AowError::Invalid(_) | AowError::Git(_) | AowError::Timeout(_) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_aow_request",
            error.to_string(),
            None,
        ),
        AowError::ProjectNotFound(_) | AowError::AgentNotFound(_) => HttpError::new(
            StatusCode::NOT_FOUND,
            "aow_item_not_found",
            error.to_string(),
            None,
        ),
        AowError::Io(error) if error.kind() == std::io::ErrorKind::AlreadyExists => HttpError::new(
            StatusCode::CONFLICT,
            "notes_entry_exists",
            error.to_string(),
            None,
        ),
        AowError::Io(error) => error.into(),
        AowError::Poisoned | AowError::Json(_) | AowError::Configuration(_) => {
            HttpError::internal(error.to_string())
        }
    }
}

pub(super) fn aow_response(error: AowError) -> Response {
    aow_http_error(error).into_response()
}

pub(super) fn snapshot_response(error: aow_agents::sessions::snapshot::SnapshotError) -> Response {
    match error {
        aow_agents::sessions::snapshot::SnapshotError::NotFound => HttpError::new(
            StatusCode::NOT_FOUND,
            "agent_session_not_found",
            error.to_string(),
            None,
        ),
        aow_agents::sessions::snapshot::SnapshotError::Invalid(_) => HttpError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_agent_session",
            error.to_string(),
            None,
        ),
        aow_agents::sessions::snapshot::SnapshotError::Io(_)
        | aow_agents::sessions::snapshot::SnapshotError::Database(_) => {
            HttpError::internal(error.to_string())
        }
    }
    .into_response()
}
