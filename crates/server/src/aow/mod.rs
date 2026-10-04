use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use aow_agents::environment::SETTINGS_FILE;
pub(crate) use aow_agents::environment::discover_path as discovery_path;
pub(crate) use aow_agents::launch::AgentLaunch;
use aow_agents::launch::{AgentRegistration, AgentType};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{AppState, HttpError};
use git::git_output;

mod agents;
mod api;
mod configuration;
mod error;
mod git;
mod global;
mod notes;
mod paths;
mod project_avatar;
mod projects;
mod registry;
mod removal;
mod sessions;
mod settings;
mod validation;
mod worktrees;

use sessions::CachedSessionLocator;

#[cfg(test)]
use registry::RegistryDocument;
pub(crate) use registry::atomic_save_document;
use registry::{
    AGENTS_FILE, PROJECTS_FILE, REGISTRY_VERSION, RegistryState, StoredAgent, StoredProject,
    load_registry,
};

use agents::{AgentsQuery, RegisterAgentRequest};
use notes::{CreateTemporaryNoteRequest, TemporaryNoteResult};
pub(crate) use projects::ProjectSummary;
use projects::{BindNotesRequest, Project, RegisterProjectRequest};
use sessions::{AgentSessionSnapshotQuery, AgentSessionsQuery, AutomationRunSessionQuery};
use worktrees::{
    CreateWorktreeRequest, ProjectBranches, SetWorktreeColorRequest, SetWorktreeIconRequest,
    Worktree, WorktreeColor, WorktreeIcon, WorktreeRemovalPreview, WorktreeRemovalQuery,
};

use settings::{
    AowSettings, EditorSettings, PinnedDirectories, PinnedWorktrees,
    UpdatePinnedDirectoriesRequest, UpdatePinnedWorktreesRequest, UpdateSettingsRequest,
    default_notes_base, load_settings,
};

pub(crate) use error::{AowError, aow_http_error};
use error::{aow_response, snapshot_response};

pub(crate) use agents::resolve_executable;
pub(crate) use api::{resolve_session_locator, routes};

#[derive(Clone)]
pub(crate) struct AowManager {
    inner: Arc<AowInner>,
}

struct AowInner {
    notifications: crate::notifications::NotificationManager,
    state_dir: Option<PathBuf>,
    config: Option<aow_config::ConfigRepository>,
    configuration_file: Option<Mutex<aow_config::ConfigFile>>,
    projects_path: Option<PathBuf>,
    agents_path: Option<PathBuf>,
    settings_path: Option<PathBuf>,
    state: Mutex<RegistryState>,
    settings: Mutex<AowSettings>,
    session_locators: Mutex<HashMap<String, CachedSessionLocator>>,
    project_operation: Arc<tokio::sync::Mutex<()>>,
    filesystem_operation: Arc<tokio::sync::RwLock<()>>,
    removals: removal::RemovalJobs,
}

impl AowManager {
    pub(crate) fn repository_roots(&self) -> Result<Vec<PathBuf>, AowError> {
        Ok(self
            .lock()?
            .projects
            .iter()
            .map(|project| PathBuf::from(&project.registered_path))
            .collect())
    }

    pub(crate) fn in_memory() -> Self {
        Self {
            inner: Arc::new(AowInner {
                notifications: crate::notifications::NotificationManager::in_memory(),
                state_dir: None,
                config: None,
                configuration_file: None,
                projects_path: None,
                agents_path: None,
                settings_path: None,
                state: Mutex::new(RegistryState::default()),
                settings: Mutex::new(AowSettings::with_notes_base(default_notes_base())),
                session_locators: Mutex::new(HashMap::new()),
                project_operation: Arc::new(tokio::sync::Mutex::new(())),
                filesystem_operation: Arc::new(tokio::sync::RwLock::new(())),
                removals: removal::RemovalJobs::in_memory(),
            }),
        }
    }

    pub(crate) fn persistent(state_dir: &Path) -> Result<Self, AowError> {
        Self::persistent_with_notes_base(state_dir, default_notes_base())
    }

    fn persistent_with_notes_base(state_dir: &Path, notes_base: PathBuf) -> Result<Self, AowError> {
        let config =
            aow_config::ConfigRepository::initialize(state_dir).map_err(AowError::Configuration)?;
        let state_dir = std::fs::canonicalize(state_dir)?;
        let configuration_file =
            aow_config::ConfigFile::load(&state_dir).map_err(AowError::Configuration)?;
        let projects_path = config.directory().join(PROJECTS_FILE);
        let agents_path = config.directory().join(AGENTS_FILE);
        let settings_path = config.directory().join(SETTINGS_FILE);
        let projects = load_registry(&projects_path)?;
        let agents = load_registry(&agents_path)?;
        let settings = load_settings(&settings_path, notes_base)?;
        let notifications = crate::notifications::NotificationManager::persistent(&state_dir)?;
        let removals = removal::RemovalJobs::in_memory();
        Ok(Self {
            inner: Arc::new(AowInner {
                notifications,
                state_dir: Some(state_dir),
                config: Some(config),
                configuration_file: Some(Mutex::new(configuration_file)),
                projects_path: Some(projects_path),
                agents_path: Some(agents_path),
                settings_path: Some(settings_path),
                state: Mutex::new(RegistryState { projects, agents }),
                settings: Mutex::new(settings),
                session_locators: Mutex::new(HashMap::new()),
                project_operation: Arc::new(tokio::sync::Mutex::new(())),
                filesystem_operation: Arc::new(tokio::sync::RwLock::new(())),
                removals,
            }),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, RegistryState>, AowError> {
        self.inner.state.lock().map_err(|_| AowError::Poisoned)
    }

    pub(crate) fn notifications(&self) -> &crate::notifications::NotificationManager {
        &self.inner.notifications
    }

    pub(crate) async fn filesystem_access(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.inner.filesystem_operation.read().await
    }
}

#[cfg(test)]
mod tests;
