//! Task organization is independent of runtime parent/child relationships.
use crate::error::{ServiceError, service_http_error};
use crate::{AppState, HttpError, roles};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
mod api;
mod instructions;
pub(crate) use instructions::is_orchestrator_thread;
mod mentions;
mod messages;
mod model;
mod routing;
#[cfg(test)]
mod tests;
mod workspace;

use crate::projects::worktrees::CreateWorktreeRequest;
use aow_db::tasks::TaskResource;
mod resources;
mod service;
pub(crate) use api::routes;
pub(crate) use routing::cli_routes;
pub(crate) use service::TaskService;

mod delivery;
mod workflow;
