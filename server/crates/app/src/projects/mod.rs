use crate::error::{ServiceError, service_response};
use crate::{AppState, validation};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
mod api;
mod avatar;
mod global;
mod launch;
mod model;
pub(crate) mod notes;
mod registry;
mod service;
mod sessions;
mod storage;
mod task_worktree;
pub(crate) mod worktrees;
pub(crate) use api::routes;
use notes::CreateTemporaryNoteRequest;
use notes::TemporaryNoteResult;
pub(crate) use service::ProjectService;
use storage::RegistryState;
#[cfg(test)]
pub(crate) use storage::StoredProject;
#[cfg(not(test))]
use storage::StoredProject;
use worktrees::*;

pub(crate) use task_worktree::TaskWorktreePolicy;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectSummary {
    pub(super) id: String,
    pub(super) name: String,
    pub(crate) repo_path: String,
}

impl From<&StoredProject> for ProjectSummary {
    fn from(project: &StoredProject) -> Self {
        Self {
            id: project.id.clone(),
            name: project.name.clone(),
            repo_path: project.registered_path.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Project {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) registered_path: String,
    pub(super) common_git_dir: String,
    pub(super) notes_path: String,
    pub(super) builtin: bool,
    pub(super) avatar_url: Option<String>,
    pub(super) worktrees: Vec<Worktree>,
    pub(super) task_worktree: TaskWorktreePolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterProjectRequest {
    pub(super) path: String,
    pub(super) name: Option<String>,
    pub(super) notes_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BindNotesRequest {
    pub(super) path: String,
}
