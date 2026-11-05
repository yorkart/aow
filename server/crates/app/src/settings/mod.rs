use crate::AppState;
use crate::error::{ServiceError, service_response};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod configuration;
mod manager;
mod model;
mod pinned;
mod server_environment;

pub(crate) use server_environment::routes as server_environment_routes;

pub(crate) use configuration::{UpdateSettingsRequest, default_notes_base, load_settings};
pub(crate) use model::AowSettings;
#[cfg(test)]
pub(crate) use model::EditorSettings;
pub(crate) use pinned::{
    PinnedDirectories, PinnedWorktrees, UpdatePinnedDirectoriesRequest,
    UpdatePinnedWorktreesRequest,
};

#[cfg(test)]
pub(crate) use configuration::prepare_notes_base_directory;
mod api;
pub(crate) mod persistence;
mod service;
pub(crate) use api::routes;
pub(crate) use service::SettingsService;
mod acp;
pub(crate) use acp::AcpSettingsFile;
