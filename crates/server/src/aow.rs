use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::CStr,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

pub(crate) use aow_agents::environment::discover_path as discovery_path;
use aow_agents::environment::{self, SETTINGS_FILE};
pub(crate) use aow_agents::launch::AgentLaunch;
use aow_agents::launch::{AgentRegistration, AgentType};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{fs::OpenOptions, process::Command};
use uuid::Uuid;

use crate::{AppState, HttpError};

const PROJECTS_FILE: &str = "aow-projects.json";
const AGENTS_FILE: &str = "aow-agents.json";
const REGISTRY_VERSION: u32 = 1;
mod configuration;
mod global;
mod project_avatar;
mod removal;

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
    #[error("agent not found or unavailable: {0}")]
    AgentNotFound(String),
    #[error("git command failed: {0}")]
    Git(String),
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

#[derive(Debug, Clone)]
struct CachedSessionLocator {
    worktree: PathBuf,
    locator: aow_agents::sessions::AgentSessionLocator,
}

#[derive(Default)]
struct RegistryState {
    projects: Vec<StoredProject>,
    agents: Vec<StoredAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AowSettings {
    notes_base: String,
    #[serde(default)]
    node_addresses: Vec<String>,
    #[serde(default)]
    editor: EditorSettings,
    #[serde(default)]
    execution_path: Option<Vec<PathBuf>>,
    #[serde(default)]
    pinned_worktrees: Vec<String>,
    #[serde(default)]
    pinned_worktrees_revision: u64,
    #[serde(default)]
    pinned_directories: Vec<String>,
    #[serde(default)]
    pinned_directories_revision: u64,
}

impl AowSettings {
    fn with_notes_base(notes_base: PathBuf) -> Self {
        Self {
            notes_base: notes_base.to_string_lossy().into_owned(),
            node_addresses: Vec::new(),
            editor: EditorSettings::default(),
            execution_path: None,
            pinned_worktrees: Vec::new(),
            pinned_worktrees_revision: 0,
            pinned_directories: Vec::new(),
            pinned_directories_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EditorSettings {
    #[serde(default)]
    word_wrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsDocument {
    version: u32,
    notes_base: String,
    #[serde(default)]
    node_addresses: Vec<String>,
    #[serde(default)]
    editor: EditorSettings,
    #[serde(default)]
    execution_path: Option<Vec<PathBuf>>,
    #[serde(default)]
    pinned_worktrees: Vec<String>,
    #[serde(default)]
    pinned_worktrees_revision: u64,
    #[serde(default)]
    pinned_directories: Vec<String>,
    #[serde(default)]
    pinned_directories_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PinnedDirectories {
    paths: Vec<String>,
    revision: u64,
}

#[derive(Debug, Default, Deserialize)]
struct UpdatePinnedDirectoriesRequest {
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PinnedWorktrees {
    paths: Vec<String>,
    revision: u64,
}

#[derive(Debug, Default, Deserialize)]
struct UpdatePinnedWorktreesRequest {
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
    order: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryDocument<T> {
    version: u32,
    items: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProject {
    id: String,
    name: String,
    registered_path: String,
    #[serde(default)]
    common_git_dir: String,
    notes_path: String,
    #[serde(default)]
    notes_identity: Option<PathBuf>,
    #[serde(default)]
    notes_custom: bool,
    #[serde(default)]
    builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    #[serde(default)]
    worktree_colors: BTreeMap<String, WorktreeColor>,
    #[serde(default)]
    worktree_icons: BTreeMap<String, WorktreeIcon>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAgent {
    id: String,
    #[serde(default)]
    agent_type: Option<AgentType>,
    display_name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectSummary {
    id: String,
    name: String,
    repo_path: String,
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
pub(crate) struct Project {
    id: String,
    name: String,
    registered_path: String,
    common_git_dir: String,
    notes_path: String,
    builtin: bool,
    avatar_url: Option<String>,
    worktrees: Vec<Worktree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorktreeColor {
    #[default]
    Default,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Gray,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorktreeIcon {
    #[default]
    Default,
    Cat,
    Dog,
    Rabbit,
    Bird,
    Fish,
    Turtle,
    Squirrel,
    Snail,
    Bug,
    Rat,
    Apple,
    Banana,
    Cherry,
    Citrus,
    Grape,
    Pear,
    Peach,
    Strawberry,
    Watermelon,
    Pineapple,
    Leaf,
    Sprout,
    Flower,
    #[serde(rename = "flower_2")]
    Flower2,
    Clover,
    Wheat,
    TreePine,
    TreeDeciduous,
    TreePalm,
    Trees,
}

#[derive(Debug, Clone, Serialize)]
struct Worktree {
    id: String,
    project_id: String,
    path: String,
    branch: String,
    head: String,
    is_main: bool,
    detached: bool,
    locked: bool,
    prunable: bool,
    color: WorktreeColor,
    icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
struct RegisterProjectRequest {
    path: String,
    name: Option<String>,
    notes_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BindNotesRequest {
    path: String,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateSettingsRequest {
    notes_base: Option<String>,
    node_addresses: Option<Vec<String>>,
    execution_path: Option<Vec<PathBuf>>,
    editor: Option<EditorSettings>,
}

#[derive(Debug, Deserialize)]
struct CreateTemporaryNoteRequest {
    extension: TemporaryNoteExtension,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TemporaryNoteExtension {
    Md,
    Txt,
}

impl TemporaryNoteExtension {
    fn as_str(self) -> &'static str {
        match self {
            Self::Md => "md",
            Self::Txt => "txt",
        }
    }
}

#[derive(Debug, Serialize)]
struct TemporaryNoteResult {
    path: String,
    name: String,
    kind: &'static str,
}

#[derive(Debug, Deserialize)]
struct CreateWorktreeRequest {
    branch: String,
    base_ref: String,
    path: String,
    #[serde(default)]
    pull_first: bool,
}

#[derive(Debug, Serialize)]
struct CreateWorktreeResponse {
    project: Project,
    worktree: Worktree,
}

#[derive(Debug, Deserialize)]
struct SetWorktreeColorRequest {
    path: String,
    color: WorktreeColor,
}

#[derive(Debug, Deserialize)]
struct SetWorktreeIconRequest {
    path: String,
    icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
struct WorktreeRemovalQuery {
    path: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct AgentSessionsQuery {
    worktree_path: String,
    agent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentSessionSnapshotQuery {
    agent: String,
    worktree_path: String,
}

#[derive(Debug, Deserialize)]
struct AutomationRunSessionQuery {
    task_id: String,
    run_id: String,
}

#[derive(Debug)]
struct WorktreeRemovalInspection {
    worktree: Worktree,
    changes: Vec<String>,
    change_count: usize,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct WorktreeRemovalPreview {
    worktree: Worktree,
    dirty: bool,
    changes: Vec<String>,
    change_count: usize,
    truncated: bool,
    terminal_tabs: usize,
    agent_tabs: usize,
}

#[derive(Debug, Deserialize)]
struct RegisterAgentRequest {
    id: Option<String>,
    agent_type: AgentType,
    display_name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct AgentsQuery {
    #[allow(dead_code)]
    refresh: Option<bool>,
}

impl AowManager {
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

    fn lock_settings(&self) -> Result<MutexGuard<'_, AowSettings>, AowError> {
        self.inner.settings.lock().map_err(|_| AowError::Poisoned)
    }

    fn settings(&self) -> Result<AowSettings, AowError> {
        Ok(self.lock_settings()?.clone())
    }

    pub(crate) async fn execution_path(&self) -> Result<Vec<PathBuf>, AowError> {
        if let Some(path) = self.settings()?.execution_path {
            return Ok(path);
        }
        let path = environment::normalize_path(discovery_path().await)
            .map_err(|error| AowError::Invalid(error.to_string()))?;
        let mut settings = self.lock_settings()?;
        // A concurrent Settings update wins over default discovery.
        if let Some(path) = &settings.execution_path {
            return Ok(path.clone());
        }
        settings.execution_path = Some(path.clone());
        if let Err(error) = self.persist_settings(&settings) {
            settings.execution_path = None;
            return Err(error);
        }
        Ok(path)
    }

    fn pinned_worktrees(&self) -> Result<PinnedWorktrees, AowError> {
        let settings = self.lock_settings()?;
        Ok(PinnedWorktrees {
            paths: settings.pinned_worktrees.clone(),
            revision: settings.pinned_worktrees_revision,
        })
    }

    fn update_pinned_worktrees(
        &self,
        request: UpdatePinnedWorktreesRequest,
    ) -> Result<PinnedWorktrees, AowError> {
        for path in request
            .add
            .iter()
            .chain(&request.remove)
            .chain(request.order.iter().flatten())
        {
            if !Path::new(path).is_absolute() || path.contains('\0') {
                return Err(AowError::Invalid(
                    "pinned worktree must be an absolute path".to_owned(),
                ));
            }
        }
        let mut settings = self.lock_settings()?;
        let previous = settings.clone();
        settings
            .pinned_worktrees
            .retain(|path| !request.remove.contains(path));
        for path in request.add {
            if !settings.pinned_worktrees.contains(&path) {
                settings.pinned_worktrees.push(path);
            }
        }
        if let Some(order) = request.order {
            let mut ordered = Vec::new();
            for path in order {
                if settings.pinned_worktrees.contains(&path) && !ordered.contains(&path) {
                    ordered.push(path);
                }
            }
            for path in &settings.pinned_worktrees {
                if !ordered.contains(path) {
                    ordered.push(path.clone());
                }
            }
            settings.pinned_worktrees = ordered;
        }
        if settings.pinned_worktrees != previous.pinned_worktrees {
            settings.pinned_worktrees_revision += 1;
            if let Err(error) = self.persist_settings(&settings) {
                *settings = previous;
                return Err(error);
            }
        }
        Ok(PinnedWorktrees {
            paths: settings.pinned_worktrees.clone(),
            revision: settings.pinned_worktrees_revision,
        })
    }

    fn pinned_directories(&self) -> Result<PinnedDirectories, AowError> {
        let settings = self.lock_settings()?;
        Ok(PinnedDirectories {
            paths: settings.pinned_directories.clone(),
            revision: settings.pinned_directories_revision,
        })
    }

    async fn update_pinned_directories(
        &self,
        request: UpdatePinnedDirectoriesRequest,
    ) -> Result<PinnedDirectories, AowError> {
        let normalize = |path: String| -> Result<String, AowError> {
            if !Path::new(&path).is_absolute() || path.contains('\0') {
                return Err(AowError::Invalid(
                    "pinned directory must be an absolute path".to_owned(),
                ));
            }
            let path = path.trim_end_matches('/');
            Ok(if path.is_empty() { "/" } else { path }.to_owned())
        };
        let add = request
            .add
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()?;
        let remove = request
            .remove
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()?;
        for path in &add {
            if !tokio::fs::metadata(path).await?.is_dir() {
                return Err(AowError::Invalid(format!("not a directory: {path}")));
            }
        }
        // Removal also works for bookmarks whose directory was moved or deleted.
        let mut settings = self.lock_settings()?;
        let previous = settings.clone();
        settings
            .pinned_directories
            .retain(|path| !remove.contains(path));
        for path in add {
            if !settings.pinned_directories.contains(&path) {
                settings.pinned_directories.push(path);
            }
        }
        if settings.pinned_directories != previous.pinned_directories {
            settings.pinned_directories_revision += 1;
            if let Err(error) = self.persist_settings(&settings) {
                *settings = previous;
                return Err(error);
            }
        }
        Ok(PinnedDirectories {
            paths: settings.pinned_directories.clone(),
            revision: settings.pinned_directories_revision,
        })
    }

    async fn update_settings(
        &self,
        request: UpdateSettingsRequest,
    ) -> Result<AowSettings, AowError> {
        let node_addresses = request
            .node_addresses
            .map(normalize_node_addresses)
            .transpose()?;
        let execution_path = request
            .execution_path
            .map(environment::normalize_path)
            .transpose()
            .map_err(|error| AowError::Invalid(error.to_string()))?;
        let notes_base = if let Some(notes_base) = request.notes_base {
            Some(prepare_notes_base_directory(Path::new(notes_base.trim())).await?)
        } else {
            None
        };
        if execution_path.is_none() {
            self.execution_path().await?;
        }
        let operation = self.inner.project_operation.clone().lock_owned().await;
        self.update_notes_settings(
            notes_base,
            execution_path,
            request.editor,
            node_addresses,
            operation,
        )
        .await
    }

    /// Registry metadata only; callers discover worktrees directly through Git.
    pub(crate) fn registered_projects(&self) -> Result<Vec<ProjectSummary>, AowError> {
        Ok(self
            .lock()?
            .projects
            .iter()
            .map(ProjectSummary::from)
            .collect())
    }

    pub(crate) fn registered_project(&self, id: &str) -> Result<ProjectSummary, AowError> {
        self.lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .map(ProjectSummary::from)
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))
    }

    async fn projects(&self) -> Result<Vec<Project>, AowError> {
        let stored = self.lock()?.projects.clone();
        let mut projects = Vec::with_capacity(stored.len());
        for item in stored {
            projects.push(project_from_stored(item).await);
        }
        Ok(projects)
    }

    /// Resolve notification labels from the registered project, including
    /// linked worktrees whose directory name differs from the project name.
    pub(crate) async fn project_name_for_workspace(&self, root: &str) -> Option<String> {
        let projects = self.lock().ok()?.projects.clone();
        if let Some(project) = projects
            .iter()
            .find(|project| project.registered_path == root)
        {
            return Some(project.name.clone());
        }
        if projects.is_empty() {
            return None;
        }
        let common = resolve_common_git_dir(Path::new(root)).await.ok()?;
        projects
            .into_iter()
            .find(|project| project.common_git_dir == common)
            .map(|project| project.name)
    }

    async fn register_project(&self, request: RegisterProjectRequest) -> Result<Project, AowError> {
        let _operation = self.inner.project_operation.lock().await;
        self.register_project_inner(request).await
    }

    async fn register_project_inner(
        &self,
        request: RegisterProjectRequest,
    ) -> Result<Project, AowError> {
        let input = validate_absolute_directory(&request.path).await?;
        let root = git_output(&input, &["rev-parse", "--show-toplevel"]).await?;
        let root = canonical_directory(Path::new(root.trim())).await?;
        let registered_path = root.to_string_lossy().into_owned();
        let common_git_dir = resolve_common_git_dir(&root).await?;
        let default_name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Project")
            .to_owned();
        let requested_name = request
            .name
            .as_deref()
            .map(validate_display_name)
            .transpose()?;
        let existing_notes_path = self
            .lock()?
            .projects
            .iter()
            .find(|project| {
                project.registered_path == registered_path
                    || (!project.common_git_dir.is_empty()
                        && project.common_git_dir == common_git_dir)
            })
            .map(|project| project.notes_path.clone());
        let notes_identity = if request.notes_path.is_none() && existing_notes_path.is_none() {
            Some(default_notes_identity(&root).await?)
        } else {
            None
        };
        let notes_path = if let Some(path) = request.notes_path.as_deref() {
            prepare_notes_directory(Path::new(path)).await?
        } else if let Some(existing_notes_path) = existing_notes_path {
            existing_notes_path
        } else {
            let base = PathBuf::from(self.settings()?.notes_base);
            let identity = default_notes_identity(&root).await?;
            prepare_notes_directory(&base.join(identity)).await?
        };

        let item = {
            let mut state = self.lock()?;
            if let Some(existing) = state.projects.iter_mut().find(|project| {
                project.registered_path == registered_path
                    || (!project.common_git_dir.is_empty()
                        && project.common_git_dir == common_git_dir)
            }) {
                if let Some(name) = requested_name {
                    if !existing.builtin {
                        existing.name = name;
                    }
                }
                if request.notes_path.is_some() {
                    existing.notes_identity = None;
                    existing.notes_custom = true;
                }
                existing.registered_path = registered_path;
                existing.common_git_dir = common_git_dir;
                existing.notes_path = notes_path;
                let result = existing.clone();
                self.persist_projects(&state.projects)?;
                result
            } else {
                let project = StoredProject {
                    id: Uuid::new_v4().to_string(),
                    name: requested_name.unwrap_or_else(|| default_name.clone()),
                    registered_path,
                    common_git_dir,
                    notes_path,
                    notes_identity,
                    notes_custom: request.notes_path.is_some(),
                    builtin: false,
                    avatar_url: None,
                    worktree_colors: BTreeMap::new(),
                    worktree_icons: BTreeMap::new(),
                };
                state.projects.push(project.clone());
                self.persist_projects(&state.projects)?;
                project
            }
        };
        project_from_stored_strict(item).await
    }

    async fn bind_notes(&self, id: &str, path: &str) -> Result<Project, AowError> {
        let _operation = self.inner.project_operation.lock().await;
        self.lock()?
            .projects
            .iter()
            .any(|project| project.id == id)
            .then_some(())
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let notes_path = prepare_notes_directory(Path::new(path)).await?;
        let item = {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index].clone();
            state.projects[index].notes_path = notes_path;
            state.projects[index].notes_identity = None;
            state.projects[index].notes_custom = true;
            let item = state.projects[index].clone();
            if let Err(error) = self.persist_projects(&state.projects) {
                state.projects[index] = previous;
                return Err(error);
            }
            item
        };
        project_from_stored_strict(item).await
    }

    async fn remove_project(&self, id: &str) -> Result<(), AowError> {
        let _operation = self.inner.project_operation.lock().await;
        let mut state = self.lock()?;
        if self.inner.removals.project_busy(id)? {
            return Err(AowError::RemovalConflict(
                "Project 正在清理 Worktree，请等待完成".into(),
            ));
        }
        let index = state
            .projects
            .iter()
            .position(|project| project.id == id)
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        if state.projects[index].builtin {
            return Err(AowError::Invalid("内置浮动工作区不能移除".into()));
        }
        let removed = state.projects.remove(index);
        if let Err(error) = self.persist_projects(&state.projects) {
            state.projects.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }

    async fn project(&self, id: &str) -> Result<Project, AowError> {
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        project_from_stored_strict(stored).await
    }

    async fn create_worktree(
        &self,
        id: &str,
        request: CreateWorktreeRequest,
    ) -> Result<CreateWorktreeResponse, AowError> {
        let project_lock = self.inner.removals.project_lock(id)?;
        let _project = project_lock.lock().await;
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let branch = validate_git_value("branch", &request.branch)?;
        let base_ref = validate_git_value("base ref", &request.base_ref)?;
        let path = validate_new_worktree_path(&request.path).await?;
        let registered_path = PathBuf::from(&stored.registered_path);

        git_output(&registered_path, &["check-ref-format", "--branch", &branch]).await?;
        if request.pull_first {
            let output = git_output(&registered_path, &["worktree", "list", "--porcelain"]).await?;
            let main_worktree = parse_worktrees(id, &output)
                .into_iter()
                .find(|worktree| worktree.is_main)
                .ok_or_else(|| AowError::Git("project has no main worktree".to_owned()))?;
            git_output(Path::new(&main_worktree.path), &["pull"])
                .await
                .map_err(|error| {
                    AowError::Git(format!(
                        "git pull failed in main worktree {}: {error}",
                        main_worktree.path
                    ))
                })?;
        }
        let path_value = path.to_string_lossy().into_owned();
        git_output(
            &registered_path,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                "--",
                &path_value,
                &base_ref,
            ],
        )
        .await?;

        let created_path = canonical_directory(&path).await?;
        let project = project_from_stored_strict(stored).await?;
        let worktree = project
            .worktrees
            .iter()
            .find(|worktree| Path::new(&worktree.path) == created_path)
            .cloned()
            .ok_or_else(|| {
                AowError::Git(format!(
                    "created worktree was not returned by git worktree list: {}",
                    created_path.display()
                ))
            })?;
        Ok(CreateWorktreeResponse { project, worktree })
    }

    async fn set_worktree_color(
        &self,
        id: &str,
        request: SetWorktreeColorRequest,
    ) -> Result<Project, AowError> {
        let mut project = self.project(id).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| worktree.path == request.path)
        {
            return Err(AowError::Invalid(format!(
                "path is not a worktree in project {}: {}",
                project.name, request.path
            )));
        }

        {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index]
                .worktree_colors
                .get(&request.path)
                .copied();
            if request.color == WorktreeColor::Default {
                state.projects[index].worktree_colors.remove(&request.path);
            } else {
                state.projects[index]
                    .worktree_colors
                    .insert(request.path.clone(), request.color);
            }
            if let Err(error) = self.persist_projects(&state.projects) {
                match previous {
                    Some(color) => {
                        state.projects[index]
                            .worktree_colors
                            .insert(request.path, color);
                    }
                    None => {
                        state.projects[index].worktree_colors.remove(&request.path);
                    }
                }
                return Err(error);
            }
        }
        if let Some(worktree) = project
            .worktrees
            .iter_mut()
            .find(|worktree| worktree.path == request.path)
        {
            worktree.color = request.color;
        }
        Ok(project)
    }

    async fn set_worktree_icon(
        &self,
        id: &str,
        request: SetWorktreeIconRequest,
    ) -> Result<Project, AowError> {
        let mut project = self.project(id).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| worktree.path == request.path)
        {
            return Err(AowError::Invalid(format!(
                "path is not a worktree in project {}: {}",
                project.name, request.path
            )));
        }

        {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index]
                .worktree_icons
                .get(&request.path)
                .copied();
            if request.icon == WorktreeIcon::Default {
                state.projects[index].worktree_icons.remove(&request.path);
            } else {
                state.projects[index]
                    .worktree_icons
                    .insert(request.path.clone(), request.icon);
            }
            if let Err(error) = self.persist_projects(&state.projects) {
                match previous {
                    Some(icon) => {
                        state.projects[index]
                            .worktree_icons
                            .insert(request.path, icon);
                    }
                    None => {
                        state.projects[index].worktree_icons.remove(&request.path);
                    }
                }
                return Err(error);
            }
        }
        if let Some(worktree) = project
            .worktrees
            .iter_mut()
            .find(|worktree| worktree.path == request.path)
        {
            worktree.icon = request.icon;
        }
        Ok(project)
    }

    async fn inspect_worktree_removal(
        &self,
        id: &str,
        requested_path: &str,
    ) -> Result<WorktreeRemovalInspection, AowError> {
        let (_, _, worktree, _) = self.worktree_removal_target(id, requested_path).await?;
        let status = git_output(
            Path::new(&worktree.path),
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .await?;
        const MAX_REPORTED_CHANGES: usize = 100;
        let mut all_changes = status.lines().map(str::to_owned).collect::<Vec<_>>();
        let change_count = all_changes.len();
        let truncated = change_count > MAX_REPORTED_CHANGES;
        all_changes.truncate(MAX_REPORTED_CHANGES);
        Ok(WorktreeRemovalInspection {
            worktree,
            changes: all_changes,
            change_count,
            truncated,
        })
    }

    #[cfg(test)]
    async fn remove_worktree(
        &self,
        id: &str,
        requested_path: &str,
        force: bool,
    ) -> Result<Project, AowError> {
        self.remove_worktree_files(id, requested_path, force)
            .await?;
        self.project(id).await
    }

    async fn remove_worktree_files(
        &self,
        id: &str,
        requested_path: &str,
        force: bool,
    ) -> Result<(), AowError> {
        let (stored, _, worktree, git_cwd) =
            self.worktree_removal_target(id, requested_path).await?;
        let status = git_output(
            Path::new(&worktree.path),
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .await?;
        let dirty_count = status.lines().count();
        if dirty_count > 0 && !force {
            return Err(AowError::DirtyWorktree(dirty_count));
        }

        let (previous_registered_path, previous_color, previous_icon) = {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous_registered_path =
                (Path::new(&stored.registered_path) == Path::new(&worktree.path)).then(|| {
                    std::mem::replace(
                        &mut state.projects[index].registered_path,
                        git_cwd.to_string_lossy().into_owned(),
                    )
                });
            let previous_color = state.projects[index].worktree_colors.remove(&worktree.path);
            let previous_icon = state.projects[index].worktree_icons.remove(&worktree.path);
            if previous_registered_path.is_some()
                || previous_color.is_some()
                || previous_icon.is_some()
            {
                if let Err(error) = self.persist_projects(&state.projects) {
                    if let Some(previous) = previous_registered_path {
                        state.projects[index].registered_path = previous;
                    }
                    if let Some(color) = previous_color {
                        state.projects[index]
                            .worktree_colors
                            .insert(worktree.path.clone(), color);
                    }
                    if let Some(icon) = previous_icon {
                        state.projects[index]
                            .worktree_icons
                            .insert(worktree.path.clone(), icon);
                    }
                    return Err(error);
                }
            }
            (previous_registered_path, previous_color, previous_icon)
        };

        let path = worktree.path.as_str();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.extend(["--", path]);
        if let Err(error) = git_output(&git_cwd, &args).await {
            if previous_registered_path.is_some()
                || previous_color.is_some()
                || previous_icon.is_some()
            {
                let mut state = self.lock()?;
                if let Some(project) = state.projects.iter_mut().find(|project| project.id == id) {
                    if let Some(previous) = previous_registered_path {
                        project.registered_path = previous;
                    }
                    if let Some(color) = previous_color {
                        project.worktree_colors.insert(worktree.path.clone(), color);
                    }
                    if let Some(icon) = previous_icon {
                        project.worktree_icons.insert(worktree.path.clone(), icon);
                    }
                    self.persist_projects(&state.projects)?;
                }
            }
            return Err(error);
        }
        Ok(())
    }

    async fn worktree_removal_target(
        &self,
        id: &str,
        requested_path: &str,
    ) -> Result<(StoredProject, Project, Worktree, PathBuf), AowError> {
        let requested_path = requested_path.trim();
        if requested_path.is_empty()
            || requested_path.len() > 4096
            || requested_path.contains('\0')
            || !Path::new(requested_path).is_absolute()
        {
            return Err(AowError::Invalid(
                "worktree path must be an absolute path".to_owned(),
            ));
        }
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let project = project_from_stored_strict(stored.clone()).await?;
        let worktree = project
            .worktrees
            .iter()
            .find(|worktree| worktree.path == requested_path)
            .cloned()
            .ok_or_else(|| {
                AowError::Invalid(format!(
                    "path is not a worktree in project {}: {requested_path}",
                    project.name
                ))
            })?;
        if worktree.is_main {
            return Err(AowError::Invalid(
                "the main worktree cannot be removed".to_owned(),
            ));
        }
        if worktree.locked {
            return Err(AowError::Invalid(
                "locked worktrees must be unlocked before removal".to_owned(),
            ));
        }
        let git_cwd = project
            .worktrees
            .iter()
            .find(|candidate| candidate.is_main)
            .map(|candidate| PathBuf::from(&candidate.path))
            .ok_or_else(|| AowError::Git("project has no main worktree".to_owned()))?;
        Ok((stored, project, worktree, git_cwd))
    }

    async fn agents(&self) -> Result<Vec<AgentRegistration>, AowError> {
        let path = self.execution_path().await?;
        self.agents_in_path(&path)
    }

    fn agents_in_path(&self, path: &[PathBuf]) -> Result<Vec<AgentRegistration>, AowError> {
        let custom = self.lock()?.agents.clone();
        let custom_ids = custom
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<HashSet<_>>();
        let mut agents = Vec::new();
        for agent in custom {
            let agent_type = agent.agent_type.or_else(|| AgentType::from_id(&agent.id));
            let executable = resolve_executable(&agent.command, path);
            agents.push(AgentRegistration {
                id: agent.id,
                agent_type,
                display_name: agent.display_name,
                source: "configured",
                available: agent_type.is_some() && executable.is_some(),
                command: agent.command,
                executable: executable.map(|path| path.to_string_lossy().into_owned()),
                args: agent.args,
                env: agent.env,
            });
        }
        for agent_type in AgentType::ALL {
            let known = agent_type.agent().definition();
            if custom_ids.contains(known.id) {
                continue;
            }
            let executable = known
                .commands
                .iter()
                .find_map(|command| resolve_executable(command, path).map(|path| (*command, path)));
            if let Some((command, executable)) = executable {
                agents.push(AgentRegistration {
                    id: known.id.to_owned(),
                    agent_type: Some(agent_type),
                    display_name: known.display_name.to_owned(),
                    source: "detected",
                    available: true,
                    command: command.to_owned(),
                    executable: Some(executable.to_string_lossy().into_owned()),
                    args: known.args.iter().map(|value| (*value).to_owned()).collect(),
                    env: BTreeMap::new(),
                });
            }
        }
        agents.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(agents)
    }

    async fn register_agent(
        &self,
        request: RegisterAgentRequest,
    ) -> Result<AgentRegistration, AowError> {
        let display_name = validate_display_name(&request.display_name)?;
        let command = validate_command(&request.command)?;
        validate_arguments(&request.args)?;
        validate_env_keys(&request.env.keys().cloned().collect::<Vec<_>>())?;
        if request
            .env
            .values()
            .any(|value| value.len() > 32768 || value.contains('\0'))
        {
            return Err(AowError::Invalid(
                "agent environment values are invalid".to_owned(),
            ));
        }
        let id = request
            .id
            .as_deref()
            .map(validate_id)
            .transpose()?
            .unwrap_or_else(|| format!("custom-{}", Uuid::new_v4().as_simple()));
        if AgentType::from_id(&id).is_some_and(|agent_type| agent_type != request.agent_type) {
            return Err(AowError::Invalid(
                "内置 Agent 的类型必须与其 ID 一致；其他类型请注册为新配置".to_owned(),
            ));
        }
        let stored = StoredAgent {
            id: id.clone(),
            agent_type: Some(request.agent_type),
            display_name,
            command,
            args: request.args,
            env: request.env,
        };
        {
            let mut state = self.lock()?;
            let mut agents = state.agents.clone();
            if let Some(existing) = agents.iter_mut().find(|agent| agent.id == id) {
                *existing = stored;
            } else {
                agents.push(stored);
            }
            self.persist_agents(&agents)?;
            state.agents = agents;
        }
        self.agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id)
            .ok_or(AowError::AgentNotFound(id))
    }

    fn remove_agent(&self, id: &str) -> Result<(), AowError> {
        let mut state = self.lock()?;
        let index = state
            .agents
            .iter()
            .position(|agent| agent.id == id)
            .ok_or_else(|| AowError::AgentNotFound(id.to_owned()))?;
        let removed = state.agents.remove(index);
        if let Err(error) = self.persist_agents(&state.agents) {
            state.agents.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) async fn automation_project(
        &self,
        id: &str,
        worktree_path: &std::path::Path,
    ) -> Result<(String, PathBuf), AowError> {
        let project = self.project(id).await?;
        let requested = canonical_directory(worktree_path).await?;
        if project.error.is_some()
            || !project
                .worktrees
                .iter()
                .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(AowError::Invalid("工作区不属于所选项目".into()));
        }
        Ok((project.name, PathBuf::from(project.registered_path)))
    }

    pub(crate) async fn resolve_agent_launch(
        &self,
        id: &str,
        worktree_path: &str,
    ) -> Result<AgentLaunch, AowError> {
        let requested = canonical_directory(Path::new(worktree_path)).await?;
        let projects = self.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(AowError::Invalid(format!(
                "agent cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        let path = self.execution_path().await?;
        let agent = self
            .agents_in_path(&path)?
            .into_iter()
            .find(|agent| agent.id == id && agent.available)
            .ok_or_else(|| AowError::AgentNotFound(id.to_owned()))?;
        Ok(agent.into_launch(&path)?)
    }

    pub(crate) async fn resolve_terminal_rebuild_launch(
        &self,
        pane: &aow_protocol::TerminalPane,
        workspace_root: &str,
    ) -> Result<AgentLaunch, AowError> {
        let profile_id =
            if let Some(id) = &pane.agent_profile_id {
                id.clone()
            } else {
                // Older metadata only saved the product type. Recover the profile
                // only when its executable and full launch arguments identify it.
                let candidates: Vec<_> = self.agents().await?.into_iter().filter(|agent| {
                let Some(kind) = agent.agent_type else {
                    return false;
                };
                if !agent.available
                    || Some(kind.id()) != pane.agent_id.as_deref()
                    || agent.executable.as_deref() != Some(pane.shell.as_str())
                {
                    return false;
                }
                if agent.args == pane.arguments {
                    return true;
                }
                let suffix = pane.arguments.strip_prefix(agent.args.as_slice());
                let resume = if kind.id() == "claude" { "--resume" } else { "resume" };
                matches!(suffix, Some([flag, session]) if flag == resume && !session.is_empty())
            }).collect();
                if candidates.len() != 1 {
                    return Err(AowError::Invalid(
                        "无法唯一确定该终端原来的 Agent 配置，请检查 Agent 配置后重试".into(),
                    ));
                }
                candidates[0].id.clone()
            };
        let launch = self
            .resolve_agent_launch(&profile_id, workspace_root)
            .await?;
        if Some(launch.agent_type.id()) != pane.agent_id.as_deref() {
            return Err(AowError::Invalid(
                "原 Agent 配置的类型已改变，无法重建".into(),
            ));
        }
        Ok(launch)
    }

    pub(crate) async fn agent_worktree_path(
        &self,
        project_id: &str,
        worktree_path: &str,
    ) -> Result<String, AowError> {
        let project = self.project(project_id).await?;
        let requested = canonical_directory(Path::new(worktree_path)).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(AowError::Invalid(format!(
                "agent cwd is not a worktree of project {project_id}: {}",
                requested.display()
            )));
        }
        Ok(requested.to_string_lossy().into_owned())
    }

    async fn registered_worktree_path(&self, worktree_path: &str) -> Result<PathBuf, AowError> {
        let requested = canonical_directory(Path::new(worktree_path)).await?;
        let projects = self.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(AowError::Invalid(format!(
                "session cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        Ok(requested)
    }

    fn replace_session_locators_for_agent(
        &self,
        worktree: &Path,
        agent: Option<&str>,
        sessions: &[aow_agents::sessions::AgentSession],
    ) -> Result<(), AowError> {
        let mut locators = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?;
        locators.retain(|_, locator| {
            locator.worktree != worktree
                || agent.is_some_and(|agent| locator.locator.agent != agent)
        });
        for session in sessions {
            let locator = session.locator();
            locators.insert(
                session_locator_key(worktree, locator.agent, &locator.session_id),
                CachedSessionLocator {
                    worktree: worktree.to_path_buf(),
                    locator,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn cache_session_locator(
        &self,
        worktree: &Path,
        session: &aow_agents::sessions::AgentSession,
    ) -> Result<(), AowError> {
        let mut locator = session.locator();
        locator.cwd = std::fs::canonicalize(&locator.cwd).unwrap_or(locator.cwd);
        self.inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .insert(
                session_locator_key(worktree, locator.agent, &locator.session_id),
                CachedSessionLocator {
                    worktree: worktree.to_path_buf(),
                    locator,
                },
            );
        Ok(())
    }

    fn session_locator(
        &self,
        worktree: &Path,
        agent: &str,
        session_id: &str,
    ) -> Result<aow_agents::sessions::AgentSessionLocator, AowError> {
        let locator = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .get(&session_locator_key(worktree, agent, session_id))
            .filter(|cached| {
                cached.worktree == worktree && cached.locator.cwd.starts_with(worktree)
            })
            .map(|cached| cached.locator.clone())
            .ok_or_else(|| {
                AowError::Invalid("agent session is not in the scanned worktree".to_owned())
            })?;
        Ok(locator)
    }

    fn persist_projects(&self, projects: &[StoredProject]) -> Result<(), AowError> {
        if let Some(path) = &self.inner.projects_path {
            self.save_configuration(
                path,
                &RegistryDocument {
                    version: REGISTRY_VERSION,
                    items: projects.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    fn persist_agents(&self, agents: &[StoredAgent]) -> Result<(), AowError> {
        if let Some(path) = &self.inner.agents_path {
            self.save_configuration(
                path,
                &RegistryDocument {
                    version: REGISTRY_VERSION,
                    items: agents.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    fn persist_settings(&self, settings: &AowSettings) -> Result<(), AowError> {
        if let Some(path) = &self.inner.settings_path {
            self.save_configuration(
                path,
                &SettingsDocument {
                    version: REGISTRY_VERSION,
                    notes_base: settings.notes_base.clone(),
                    node_addresses: settings.node_addresses.clone(),
                    editor: settings.editor.clone(),
                    execution_path: settings.execution_path.clone(),
                    pinned_worktrees: settings.pinned_worktrees.clone(),
                    pinned_worktrees_revision: settings.pinned_worktrees_revision,
                    pinned_directories: settings.pinned_directories.clone(),
                    pinned_directories_revision: settings.pinned_directories_revision,
                },
            )?;
        }
        Ok(())
    }

    fn save_configuration<T: Serialize>(&self, path: &Path, document: &T) -> Result<(), AowError> {
        if let Some(config) = &self.inner.config {
            let relative = path
                .strip_prefix(config.directory())
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            let mut bytes = serde_json::to_vec_pretty(document)?;
            bytes.push(b'\n');
            config
                .save(relative, &bytes)
                .map_err(AowError::Configuration)?;
        }
        Ok(())
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .merge(configuration::routes())
        .route("/api/aow/settings", get(get_settings).put(update_settings))
        .route(
            "/api/aow/settings/discovered-path",
            get(get_discovered_path),
        )
        .route(
            "/api/aow/pinned-worktrees",
            get(get_pinned_worktrees).patch(update_pinned_worktrees),
        )
        .route(
            "/api/aow/pinned-directories",
            get(get_pinned_directories).patch(update_pinned_directories),
        )
        .route(
            "/api/aow/projects",
            get(list_projects).post(register_project),
        )
        .route("/api/aow/projects/{id}", delete(remove_project))
        .route("/api/aow/projects/{id}/refresh", post(refresh_project))
        .route("/api/aow/projects/{id}/avatar", get(project_avatar::get_avatar))
        .route(
            "/api/aow/projects/{id}/notes/bind",
            post(bind_project_notes),
        )
        .route(
            "/api/aow/projects/{id}/notes/temporary",
            post(create_temporary_project_note),
        )
        .route("/api/aow/projects/{id}/worktrees", post(create_worktree))
        .route(
            "/api/aow/projects/{id}/worktrees/color",
            post(set_worktree_color),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/icon",
            post(set_worktree_icon),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/removal",
            get(inspect_worktree_removal).delete(remove_worktree),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/removals",
            post(removal::submit_batch),
        )
        .route("/api/aow/worktree-removals", get(removal::list_jobs))
        .route("/api/aow/agents", get(list_agents).post(register_agent))
        .route("/api/aow/agents/{id}", delete(remove_agent))
        .route("/api/aow/agent-sessions", get(list_agent_sessions))
        .route(
            "/api/aow/agent-sessions/automation-run",
            get(automation_run_session),
        )
        .route(
            "/api/aow/agent-sessions/{session_id}/snapshot",
            get(agent_session_snapshot),
        )
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<AowSettings>, Response> {
    state.aow.execution_path().await.map_err(aow_response)?;
    state.aow.settings().map(Json).map_err(aow_response)
}

async fn get_discovered_path() -> Json<Vec<PathBuf>> {
    Json(discovery_path().await)
}

async fn get_pinned_worktrees(
    State(state): State<AppState>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state.aow.pinned_worktrees().map(Json).map_err(aow_response)
}

async fn update_pinned_worktrees(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedWorktreesRequest>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state
        .aow
        .update_pinned_worktrees(request)
        .map(Json)
        .map_err(aow_response)
}

async fn get_pinned_directories(
    State(state): State<AppState>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .aow
        .pinned_directories()
        .map(Json)
        .map_err(aow_response)
}

async fn update_pinned_directories(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedDirectoriesRequest>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .aow
        .update_pinned_directories(request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn update_settings(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<AowSettings>, Response> {
    state
        .aow
        .update_settings(request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, Response> {
    state.aow.projects().await.map(Json).map_err(aow_response)
}

async fn register_project(
    State(state): State<AppState>,
    Json(request): Json<RegisterProjectRequest>,
) -> Result<impl IntoResponse, Response> {
    let project = state
        .aow
        .register_project(request)
        .await
        .map_err(aow_response)?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn remove_project(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, Response> {
    state.aow.remove_project(&id).await.map_err(aow_response)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_project(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Project>, Response> {
    state.aow.project(&id).await.map(Json).map_err(aow_response)
}

async fn bind_project_notes(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<BindNotesRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .bind_notes(&id, &request.path)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn create_temporary_project_note(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateTemporaryNoteRequest>,
) -> Result<impl IntoResponse, Response> {
    let _filesystem = state.aow.filesystem_access().await;
    let root = notes_root_canonical(&state.aow, &id)
        .await
        .map_err(aow_response)?;
    let (path, name) =
        create_temporary_note_file(&root, request.extension.as_str(), short_random_id)
            .await
            .map_err(aow_response)?;
    Ok((
        StatusCode::CREATED,
        Json(TemporaryNoteResult {
            path: path.to_string_lossy().into_owned(),
            name,
            kind: "file",
        }),
    ))
}

async fn create_worktree(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateWorktreeRequest>,
) -> Result<impl IntoResponse, Response> {
    let result = state
        .aow
        .create_worktree(&id, request)
        .await
        .map_err(aow_response)?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn set_worktree_color(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SetWorktreeColorRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .set_worktree_color(&id, request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn set_worktree_icon(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SetWorktreeIconRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .set_worktree_icon(&id, request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn inspect_worktree_removal(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorktreeRemovalQuery>,
) -> Result<Json<WorktreeRemovalPreview>, Response> {
    let inspection = state
        .aow
        .inspect_worktree_removal(&id, &query.path)
        .await
        .map_err(aow_response)?;
    let (terminal_tabs, agent_tabs) = state
        .terminals
        .workspace_tab_counts(&inspection.worktree.path)
        .map_err(|error| crate::terminal::terminal_http_error(error).into_response())?;
    Ok(Json(WorktreeRemovalPreview {
        dirty: !inspection.changes.is_empty(),
        worktree: inspection.worktree,
        changes: inspection.changes,
        change_count: inspection.change_count,
        truncated: inspection.truncated,
        terminal_tabs,
        agent_tabs,
    }))
}

async fn remove_worktree(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorktreeRemovalQuery>,
) -> Result<(StatusCode, Json<removal::RemovalJob>), Response> {
    removal::submit(
        state,
        id,
        vec![removal::RemovalRequest {
            path: query.path,
            force: query.force,
        }],
    )
    .map(|job| (StatusCode::ACCEPTED, Json(job)))
    .map_err(aow_response)
}

async fn list_agents(
    State(state): State<AppState>,
    Query(_query): Query<AgentsQuery>,
) -> Result<Json<Vec<AgentRegistration>>, Response> {
    state.aow.agents().await.map(Json).map_err(aow_response)
}

async fn register_agent(
    State(state): State<AppState>,
    Json(request): Json<RegisterAgentRequest>,
) -> Result<impl IntoResponse, Response> {
    let agent = state
        .aow
        .register_agent(request)
        .await
        .map_err(aow_response)?;
    Ok((StatusCode::CREATED, Json(agent)))
}

async fn remove_agent(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, Response> {
    state.aow.remove_agent(&id).map_err(aow_response)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_agent_sessions(
    State(state): State<AppState>,
    Query(query): Query<AgentSessionsQuery>,
) -> Result<Json<Vec<aow_agents::sessions::AgentSession>>, Response> {
    let worktree = state
        .aow
        .registered_worktree_path(&query.worktree_path)
        .await
        .map_err(aow_response)?;
    let roots = aow_agents::sessions::SessionRoots::from_environment(&crate::PROCESS_HOME);
    let agent = query.agent;
    if agent.as_deref().is_some_and(|agent| {
        aow_agents::Agent::from_id(agent)
            .and_then(aow_agents::Agent::sessions)
            .is_none()
    }) {
        return Err(aow_response(AowError::Invalid(
            "unsupported agent session source".to_owned(),
        )));
    }
    let list_worktree = worktree.clone();
    let list_agent = agent.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        aow_agents::sessions::list_sessions(&list_worktree, list_agent.as_deref(), roots)
    })
    .await
    .map_err(|error| {
        HttpError::internal(format!("agent session scan failed: {error}")).into_response()
    })?;
    state
        .aow
        .replace_session_locators_for_agent(&worktree, agent.as_deref(), &sessions)
        .map_err(aow_response)?;
    Ok(Json(sessions))
}

async fn automation_run_session(
    State(state): State<AppState>,
    Query(query): Query<AutomationRunSessionQuery>,
) -> Result<Json<aow_agents::sessions::AgentSession>, Response> {
    let automations = state.automations.as_ref().ok_or_else(|| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "automations_unavailable",
            "automations require persistent state",
            None,
        )
        .into_response()
    })?;
    let run = automations
        .run(&query.task_id, &query.run_id)
        .map_err(|error| aow_response(AowError::Invalid(error.to_string())))?;
    let session_id = run.session_id.ok_or_else(|| {
        aow_response(AowError::Invalid(
            "automation run has no agent session".to_owned(),
        ))
    })?;
    let agent = run.agent.id();
    let roots = aow_agents::sessions::SessionRoots::from_environment(&crate::PROCESS_HOME);
    let found_agent = agent.to_owned();
    let found_session_id = session_id.clone();
    let session = tokio::task::spawn_blocking(move || {
        aow_agents::sessions::find_session(&found_agent, &found_session_id, roots)
    })
    .await
    .map_err(|error| {
        HttpError::internal(format!("automation agent session lookup failed: {error}"))
            .into_response()
    })?
    .ok_or_else(|| {
        HttpError::new(
            StatusCode::NOT_FOUND,
            "agent_session_not_found",
            "agent session is no longer available",
            None,
        )
        .into_response()
    })?;
    state
        .aow
        .cache_session_locator(&session.cwd_path(), &session)
        .map_err(aow_response)?;
    Ok(Json(session))
}

async fn agent_session_snapshot(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AgentSessionSnapshotQuery>,
) -> Result<Json<aow_agents::sessions::snapshot::AgentSessionSnapshot>, Response> {
    let locator =
        resolve_session_locator(&state, &session_id, &query.agent, &query.worktree_path).await?;
    tokio::task::spawn_blocking(move || aow_agents::sessions::snapshot::read(locator))
        .await
        .map_err(|error| {
            HttpError::internal(format!("agent session snapshot task failed: {error}"))
                .into_response()
        })?
        .map(Json)
        .map_err(snapshot_response)
}

pub(crate) async fn resolve_session_locator(
    state: &AppState,
    session_id: &str,
    agent: &str,
    worktree_path: &str,
) -> Result<aow_agents::sessions::AgentSessionLocator, Response> {
    if aow_agents::Agent::from_id(agent)
        .and_then(aow_agents::Agent::sessions)
        .is_none()
        || session_id.is_empty()
        || session_id.len() > 256
    {
        return Err(aow_response(AowError::Invalid(
            "invalid agent session identity".to_owned(),
        )));
    }
    // A locator is only cached by a registered-session scan or a trusted
    // automation run lookup. Once one exists, the exact workspace key is
    // enough to read its snapshot, including an automation-created worktree
    // that is intentionally not registered as a project worktree.
    let requested = Path::new(worktree_path);
    let worktree = match canonical_directory(requested).await {
        Ok(worktree) => worktree,
        Err(AowError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            removed_directory_path(requested).map_err(aow_response)?
        }
        Err(error) => return Err(aow_response(error)),
    };
    state
        .aow
        .session_locator(&worktree, agent, session_id)
        .map_err(aow_response)
}

fn session_locator_key(worktree: &Path, agent: &str, session_id: &str) -> String {
    format!("{}\0{agent}\0{session_id}", worktree.display())
}

fn removed_directory_path(path: &Path) -> Result<PathBuf, AowError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(AowError::Invalid(
            "removed session worktree path must be absolute and normalized".to_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

async fn project_from_stored(stored: StoredProject) -> Project {
    let fallback = Project {
        id: stored.id.clone(),
        name: stored.name.clone(),
        registered_path: stored.registered_path.clone(),
        common_git_dir: String::new(),
        notes_path: stored.notes_path.clone(),
        builtin: stored.builtin,
        avatar_url: stored.avatar_url.clone(),
        worktrees: Vec::new(),
        error: None,
    };
    match project_from_stored_strict(stored).await {
        Ok(project) => project,
        Err(error) => Project {
            error: Some(error.to_string()),
            ..fallback
        },
    }
}

async fn project_from_stored_strict(stored: StoredProject) -> Result<Project, AowError> {
    let registered_path = PathBuf::from(&stored.registered_path);
    let common = resolve_common_git_dir(&registered_path).await?;
    let output = git_output(&registered_path, &["worktree", "list", "--porcelain"]).await?;
    let mut worktrees = parse_worktrees(&stored.id, &output);
    for worktree in &mut worktrees {
        worktree.color = stored
            .worktree_colors
            .get(&worktree.path)
            .copied()
            .unwrap_or_default();
        worktree.icon = stored
            .worktree_icons
            .get(&worktree.path)
            .copied()
            .unwrap_or_default();
    }
    Ok(Project {
        id: stored.id,
        name: stored.name,
        registered_path: stored.registered_path,
        common_git_dir: common,
        notes_path: stored.notes_path,
        builtin: stored.builtin,
        avatar_url: stored.avatar_url,
        worktrees,
        error: None,
    })
}

async fn resolve_common_git_dir(repository: &Path) -> Result<String, AowError> {
    let common = git_output(repository, &["rev-parse", "--git-common-dir"]).await?;
    let common = PathBuf::from(common.trim());
    let common = if common.is_absolute() {
        common
    } else {
        repository.join(common)
    };
    Ok(tokio::fs::canonicalize(common)
        .await?
        .to_string_lossy()
        .into_owned())
}

fn default_notes_base() -> PathBuf {
    crate::PROCESS_HOME.join("aow")
}

async fn prepare_notes_base_directory(path: &Path) -> Result<PathBuf, AowError> {
    Ok(PathBuf::from(prepare_notes_directory(path).await?))
}

async fn repository_remote_identity(repository: &Path) -> Result<Option<PathBuf>, AowError> {
    let Some(remote) =
        optional_git_output(repository, &["config", "--get", "remote.origin.url"]).await?
    else {
        return Ok(None);
    };
    let remote = remote.trim();
    if remote.is_empty() {
        return Ok(None);
    }
    Ok(parse_remote_identity(remote).ok())
}

async fn default_notes_identity(repository: &Path) -> Result<PathBuf, AowError> {
    if let Some(identity) = repository_remote_identity(repository).await? {
        return Ok(identity);
    }
    if repository_has_remote(repository).await? {
        return Err(AowError::Invalid(
            "origin remote is required when Notes path is not provided".to_owned(),
        ));
    }
    local_repository_identity(repository, &process_account_name()?)
}

async fn repository_has_remote(repository: &Path) -> Result<bool, AowError> {
    Ok(!git_output(repository, &["remote"]).await?.trim().is_empty())
}

fn local_repository_identity(repository: &Path, account_name: &str) -> Result<PathBuf, AowError> {
    let repository_name = repository
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            AowError::Invalid("local Git repository name is not valid UTF-8".to_owned())
        })?;
    Ok(PathBuf::from("localhost")
        .join(validate_remote_component(account_name)?)
        .join(validate_remote_component(repository_name)?))
}

fn process_account_name() -> Result<String, AowError> {
    // SAFETY: The passwd record is copied into an owned String before returning.
    unsafe {
        let account = libc::getpwuid(libc::geteuid());
        if !account.is_null() && !(*account).pw_name.is_null() {
            let name = CStr::from_ptr((*account).pw_name)
                .to_string_lossy()
                .into_owned();
            if !name.is_empty() {
                return Ok(name);
            }
        }
    }
    Err(AowError::Invalid(
        "current account name is unavailable for local Notes mapping".to_owned(),
    ))
}

fn parse_remote_identity(remote: &str) -> Result<PathBuf, AowError> {
    let (authority, path) = if let Some((_, rest)) = remote.split_once("://") {
        if rest.starts_with('/') {
            return Err(AowError::Invalid(
                "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
            ));
        }
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            AowError::Invalid("origin remote URL does not include a repository path".to_owned())
        })?;
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else if let Some((authority, path)) = remote.split_once(':') {
        if authority.contains('/') || authority.contains('\\') {
            return Err(AowError::Invalid(
                "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
            ));
        }
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else {
        return Err(AowError::Invalid(
            "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
        ));
    };

    let authority = authority.trim();
    let authority = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host).unwrap_or(rest)
    } else {
        authority
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
    }
    .to_ascii_lowercase();
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| {
            path.split(['?', '#'])
                .next()
                .unwrap_or(path)
                .trim_matches('/')
        });
    if authority.is_empty() || path.is_empty() {
        return Err(AowError::Invalid(
            "origin remote URL is incomplete".to_owned(),
        ));
    }

    let mut identity = PathBuf::from(validate_remote_component(&authority)?);
    let components = path.split('/').collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(AowError::Invalid(
            "origin remote URL must include an owner and repository".to_owned(),
        ));
    }
    for component in components {
        identity.push(validate_remote_component(component)?);
    }
    Ok(identity)
}

fn validate_remote_component(value: &str) -> Result<&str, AowError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 255
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '\0'))
    {
        return Err(AowError::Invalid(
            "origin remote URL contains an unsafe path component".to_owned(),
        ));
    }
    Ok(value)
}

async fn optional_git_output(cwd: &Path, args: &[&str]) -> Result<Option<String>, AowError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| AowError::Git(error.to_string()))?;
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|error| AowError::Git(format!("git output is not UTF-8: {error}")));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(AowError::Git(if message.is_empty() {
        format!("git {} exited with {}", args.join(" "), output.status)
    } else {
        message
    }))
}

async fn prepare_notes_directory(path: &Path) -> Result<String, AowError> {
    if !path.is_absolute() {
        return Err(AowError::Invalid(
            "Notes directory must be an absolute path".to_owned(),
        ));
    }
    tokio::fs::create_dir_all(path).await?;
    Ok(canonical_directory(path)
        .await?
        .to_string_lossy()
        .into_owned())
}

async fn notes_root_canonical(manager: &AowManager, id: &str) -> Result<PathBuf, AowError> {
    let path = manager
        .lock()?
        .projects
        .iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?
        .notes_path
        .clone();
    canonical_directory(Path::new(&path)).await
}

fn short_random_id() -> String {
    const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
    let bytes = Uuid::new_v4();
    let mut value = u64::from_le_bytes(bytes.as_bytes()[..8].try_into().unwrap());
    (0..8)
        .map(|_| {
            let character = ALPHABET[(value % ALPHABET.len() as u64) as usize] as char;
            value /= ALPHABET.len() as u64;
            character
        })
        .collect()
}

async fn create_temporary_note_file(
    root: &Path,
    extension: &str,
    mut random_id: impl FnMut() -> String,
) -> Result<(PathBuf, String), AowError> {
    for _ in 0..5 {
        let name = format!(".tmp-{}.{}", random_id(), extension);
        let path = root.join(&name);
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
        {
            Ok(_) => return Ok((path, name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AowError::Invalid(
        "could not allocate a unique temporary note name after 5 attempts".to_owned(),
    ))
}

fn parse_worktrees(project_id: &str, output: &str) -> Vec<Worktree> {
    #[derive(Default)]
    struct Pending {
        path: String,
        head: String,
        branch: String,
        detached: bool,
        locked: bool,
        prunable: bool,
    }
    fn finish(project_id: &str, pending: Pending, index: usize) -> Option<Worktree> {
        if pending.path.is_empty() {
            return None;
        }
        Some(Worktree {
            id: pending.path.clone(),
            project_id: project_id.to_owned(),
            path: pending.path,
            branch: pending
                .branch
                .strip_prefix("refs/heads/")
                .unwrap_or(&pending.branch)
                .to_owned(),
            head: pending.head,
            is_main: index == 0,
            detached: pending.detached,
            locked: pending.locked,
            prunable: pending.prunable,
            color: WorktreeColor::Default,
            icon: WorktreeIcon::Default,
        })
    }

    let mut result = Vec::new();
    let mut pending = Pending::default();
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(worktree) = finish(project_id, pending, result.len()) {
                result.push(worktree);
            }
            pending = Pending::default();
        } else if let Some(value) = line.strip_prefix("worktree ") {
            pending.path = value.to_owned();
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            pending.head = value.to_owned();
        } else if let Some(value) = line.strip_prefix("branch ") {
            pending.branch = value.to_owned();
        } else if line == "detached" {
            pending.detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            pending.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            pending.prunable = true;
        }
    }
    result.sort_by(|left, right| {
        right
            .is_main
            .cmp(&left.is_main)
            .then_with(|| left.branch.cmp(&right.branch))
            .then_with(|| left.path.cmp(&right.path))
    });
    result
}

async fn git_output(cwd: &Path, args: &[&str]) -> Result<String, AowError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| AowError::Git(error.to_string()))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(AowError::Git(if message.is_empty() {
            format!("git {} exited with {}", args.join(" "), output.status)
        } else {
            message
        }));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| AowError::Git(format!("git output is not UTF-8: {error}")))
}

async fn validate_absolute_directory(value: &str) -> Result<PathBuf, AowError> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(AowError::Invalid(
            "project path must be absolute".to_owned(),
        ));
    }
    canonical_directory(path).await
}

async fn validate_new_worktree_path(value: &str) -> Result<PathBuf, AowError> {
    let value = value.trim();
    let path = PathBuf::from(value);
    if value.is_empty() || value.len() > 4096 || !path.is_absolute() || value.contains('\0') {
        return Err(AowError::Invalid(
            "worktree path must be an absolute path".to_owned(),
        ));
    }
    if tokio::fs::try_exists(&path).await? {
        return Err(AowError::Invalid(format!(
            "worktree path already exists: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        AowError::Invalid("worktree path must have a parent directory".to_owned())
    })?;
    let parent = canonical_directory(parent).await?;
    let name = path.file_name().ok_or_else(|| {
        AowError::Invalid("worktree path must include a directory name".to_owned())
    })?;
    Ok(parent.join(name))
}

fn validate_git_value(label: &str, value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(format!(
            "{label} must contain 1-1024 printable characters"
        )));
    }
    Ok(value.to_owned())
}

async fn canonical_directory(path: &Path) -> Result<PathBuf, AowError> {
    let canonical = tokio::fs::canonicalize(path).await?;
    if !tokio::fs::metadata(&canonical).await?.is_dir() {
        return Err(AowError::Invalid(format!(
            "path is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn validate_display_name(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 120 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(
            "display name must contain 1-120 printable characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn validate_id(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AowError::Invalid(
            "agent id may only contain letters, numbers, '-' and '_'".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn validate_command(value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid("agent command is invalid".to_owned()));
    }
    if value.split_whitespace().count() != 1 {
        return Err(AowError::Invalid(
            "agent command must be one executable; put flags in args".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn validate_arguments(values: &[String]) -> Result<(), AowError> {
    if values.len() > 128
        || values
            .iter()
            .any(|value| value.len() > 8192 || value.contains('\0'))
    {
        return Err(AowError::Invalid("agent arguments are invalid".to_owned()));
    }
    Ok(())
}

fn validate_env_keys(values: &[String]) -> Result<(), AowError> {
    if values.len() > 128
        || values.iter().any(|value| {
            value.is_empty()
                || value.len() > 256
                || !value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
    {
        return Err(AowError::Invalid(
            "agent environment keys are invalid".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn resolve_executable(command: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 {
        return executable_file(command_path).then(|| command_path.to_path_buf());
    }
    paths
        .iter()
        .map(|directory| directory.join(command))
        .find(|candidate| executable_file(candidate))
}

fn executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn load_registry<T>(path: &Path) -> Result<Vec<T>, AowError>
where
    T: for<'de> Deserialize<'de>,
{
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: RegistryDocument<T> = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(AowError::Invalid(format!(
                    "unsupported AoW registry version {}",
                    document.version
                )));
            }
            Ok(document.items)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn normalize_node_addresses(addresses: Vec<String>) -> Result<Vec<String>, AowError> {
    let mut normalized = Vec::new();
    for (index, address) in addresses.iter().enumerate() {
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        let url = reqwest::Url::parse(address).ok().filter(|url| {
            address
                .split_once("://")
                .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(url.scheme()))
                && matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        });
        let Some(url) = url else {
            return Err(AowError::Invalid(format!(
                "第 {} 个节点地址无效，请填写完整的 http:// 或 https:// 地址（不含用户名和密码）。",
                index + 1
            )));
        };
        let address = url.to_string();
        if !normalized.contains(&address) {
            normalized.push(address);
        }
    }
    Ok(normalized)
}

fn load_settings(path: &Path, default_notes_base: PathBuf) -> Result<AowSettings, AowError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: SettingsDocument = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(AowError::Invalid(format!(
                    "unsupported AoW settings version {}",
                    document.version
                )));
            }
            let notes_base = PathBuf::from(document.notes_base);
            if !notes_base.is_absolute() {
                return Err(AowError::Invalid(
                    "configured Notes root directory must be an absolute path".to_owned(),
                ));
            }
            Ok(AowSettings {
                notes_base: notes_base.to_string_lossy().into_owned(),
                node_addresses: document.node_addresses,
                editor: document.editor,
                execution_path: document
                    .execution_path
                    .map(environment::normalize_path)
                    .transpose()
                    .map_err(|error| AowError::Invalid(error.to_string()))?,
                pinned_worktrees: document.pinned_worktrees,
                pinned_worktrees_revision: document.pinned_worktrees_revision,
                pinned_directories: document.pinned_directories,
                pinned_directories_revision: document.pinned_directories_revision,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AowSettings::with_notes_base(default_notes_base))
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn atomic_save_document<T: Serialize>(
    path: &Path,
    document: &T,
) -> Result<(), AowError> {
    let parent = path.parent().ok_or_else(|| {
        AowError::Invalid(format!("registry path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("aow"),
        Uuid::new_v4().as_simple()
    ));
    let result = (|| {
        let bytes = serde_json::to_vec_pretty(document)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok::<_, AowError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub(crate) fn aow_http_error(error: AowError) -> HttpError {
    match error {
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
        AowError::Invalid(_) | AowError::Git(_) => HttpError::new(
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

fn aow_response(error: AowError) -> Response {
    aow_http_error(error).into_response()
}

fn snapshot_response(error: aow_agents::sessions::snapshot::SnapshotError) -> Response {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    #[tokio::test]
    async fn node_settings_persist_clear_and_preserve_other_settings() {
        let directory = tempfile::tempdir().unwrap();
        let notes_base = directory.path().join("notes");
        std::fs::write(
            directory.path().join(SETTINGS_FILE),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "notes_base": notes_base, "execution_path": ["/usr/bin"],
                "editor": { "word_wrap": true }, "pinned_worktrees": ["/repo/main"],
                "pinned_worktrees_revision": 7,
            }))
            .unwrap(),
        )
        .unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        assert!(manager.settings().unwrap().node_addresses.is_empty());
        let update = serde_json::from_value(serde_json::json!({
            "node_addresses": [" HTTPS://NODE-A.EXAMPLE:443 ", "", "https://node-a.example/",
                "http://192.168.1.2:8080/aow/?ui=desktop#tab", "http://[::1]:8080"]
        }))
        .unwrap();
        let saved = manager.update_settings(update).await.unwrap();
        let expected = vec![
            "https://node-a.example/",
            "http://192.168.1.2:8080/aow/?ui=desktop#tab",
            "http://[::1]:8080/",
        ];
        assert_eq!(saved.node_addresses, expected);
        assert_eq!(Path::new(&saved.notes_base), notes_base);
        assert!(saved.editor.word_wrap);
        assert_eq!(saved.pinned_worktrees, ["/repo/main"]);
        assert_eq!(saved.pinned_worktrees_revision, 7);
        assert_eq!(
            saved.execution_path.unwrap(),
            vec![PathBuf::from("/usr/bin")]
        );
        manager
            .update_settings(UpdateSettingsRequest {
                editor: Some(EditorSettings { word_wrap: false }),
                ..Default::default()
            })
            .await
            .unwrap();
        let reloaded = AowManager::persistent(directory.path()).unwrap();
        assert_eq!(reloaded.settings().unwrap().node_addresses, expected);
        reloaded
            .update_settings(UpdateSettingsRequest {
                node_addresses: Some(Vec::new()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            AowManager::persistent(directory.path())
                .unwrap()
                .settings()
                .unwrap()
                .node_addresses
                .is_empty()
        );
    }

    #[tokio::test]
    async fn node_settings_invalid_input_and_failed_persistence_keep_previous_value() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent_with_notes_base(
            directory.path(),
            directory.path().join("notes"),
        )
        .unwrap();
        for address in [
            "node.example",
            "javascript:alert(1)",
            "file:///tmp/node",
            "https:node/path://",
            "https://user:password@node.example",
        ] {
            assert!(
                manager
                    .update_settings(UpdateSettingsRequest {
                        node_addresses: Some(vec!["https://valid.example".into(), address.into()]),
                        ..Default::default()
                    })
                    .await
                    .is_err(),
                "accepted invalid address: {address}"
            );
            assert!(manager.settings().unwrap().node_addresses.is_empty());
        }
        std::fs::create_dir(manager.inner.settings_path.as_ref().unwrap()).unwrap();
        assert!(
            manager
                .update_settings(UpdateSettingsRequest {
                    node_addresses: Some(vec!["https://node.example".into()]),
                    execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                    ..Default::default()
                })
                .await
                .is_err()
        );
        assert!(manager.settings().unwrap().node_addresses.is_empty());
    }

    #[tokio::test]
    async fn editor_settings_default_persist_and_preserve_other_settings() {
        let directory = tempfile::tempdir().unwrap();
        let notes_base = directory.path().join("notes");
        // A settings file written before Editor preferences existed still loads.
        std::fs::write(
            directory.path().join(SETTINGS_FILE),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "notes_base": notes_base, "execution_path": ["/usr/bin"],
                "pinned_worktrees": ["/repo/main"], "pinned_worktrees_revision": 7,
            }))
            .unwrap(),
        )
        .unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        assert!(!manager.settings().unwrap().editor.word_wrap);
        let update = serde_json::from_value(serde_json::json!({
            "editor": { "word_wrap": true }
        }))
        .unwrap();
        let saved = manager.update_settings(update).await.unwrap();
        assert!(saved.editor.word_wrap);
        assert_eq!(Path::new(&saved.notes_base), notes_base);
        assert_eq!(
            saved.execution_path.unwrap(),
            vec![PathBuf::from("/usr/bin")]
        );
        assert_eq!(saved.pinned_worktrees, ["/repo/main"]);
        assert_eq!(saved.pinned_worktrees_revision, 7);

        manager
            .update_settings(UpdateSettingsRequest {
                execution_path: Some(vec![PathBuf::from("/bin")]),
                ..Default::default()
            })
            .await
            .unwrap();
        let reloaded = AowManager::persistent(directory.path()).unwrap();
        assert!(reloaded.settings().unwrap().editor.word_wrap);
        assert_eq!(
            reloaded.execution_path().await.unwrap(),
            vec![PathBuf::from("/bin")]
        );
        reloaded
            .update_settings(UpdateSettingsRequest {
                editor: Some(EditorSettings { word_wrap: false }),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            !AowManager::persistent(directory.path())
                .unwrap()
                .settings()
                .unwrap()
                .editor
                .word_wrap
        );
        assert!(
            serde_json::from_value::<UpdateSettingsRequest>(serde_json::json!({
                "editor": { "word_wrap": "on" }
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn editor_settings_failed_persistence_retains_previous_value() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent_with_notes_base(
            directory.path(),
            directory.path().join("notes"),
        )
        .unwrap();
        std::fs::create_dir(manager.inner.settings_path.as_ref().unwrap()).unwrap();
        assert!(
            manager
                .update_settings(UpdateSettingsRequest {
                    editor: Some(EditorSettings { word_wrap: true }),
                    execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                    ..Default::default()
                })
                .await
                .is_err()
        );
        assert!(!manager.settings().unwrap().editor.word_wrap);
    }

    #[tokio::test]
    async fn execution_path_defaults_persist_and_settings_updates_preserve_other_fields() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent_with_notes_base(
            directory.path(),
            directory.path().join("original notes"),
        )
        .unwrap();
        let initial = manager.execution_path().await.unwrap();
        assert!(!initial.is_empty());
        assert_eq!(
            environment::load_path(directory.path()).await.unwrap(),
            initial
        );
        let bin = directory.path().join("custom bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("codex"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(bin.join("codex"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/main".into()],
                ..Default::default()
            })
            .unwrap();
        let paths = vec![bin.clone(), PathBuf::from("/usr/bin")];
        let settings = manager
            .update_settings(UpdateSettingsRequest {
                execution_path: Some(vec![bin.clone(), bin.clone(), "/usr/bin".into()]),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(settings.execution_path.as_ref().unwrap(), &paths);
        assert_eq!(
            Path::new(&settings.notes_base),
            directory.path().join("original notes")
        );
        assert_eq!(
            manager
                .agents()
                .await
                .unwrap()
                .iter()
                .find(|agent| agent.id == "codex")
                .unwrap()
                .executable
                .as_deref(),
            bin.join("codex").to_str()
        );
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(
                    directory
                        .path()
                        .join("new notes")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ..Default::default()
            })
            .await
            .unwrap();
        let reloaded = AowManager::persistent(directory.path()).unwrap();
        assert_eq!(reloaded.execution_path().await.unwrap(), paths);
        assert_eq!(
            environment::load_path(directory.path()).await.unwrap(),
            paths
        );
        assert_eq!(reloaded.pinned_worktrees().unwrap().paths, ["/repo/main"]);
        for invalid in [
            vec![],
            vec!["relative/bin".into()],
            vec!["/bin:/usr/bin".into()],
            vec![bin.join("codex")],
        ] {
            assert!(
                manager
                    .update_settings(UpdateSettingsRequest {
                        execution_path: Some(invalid),
                        ..Default::default()
                    })
                    .await
                    .is_err()
            );
            assert_eq!(
                environment::load_path(directory.path()).await.unwrap(),
                paths
            );
        }
    }

    #[test]
    fn parses_git_worktree_porcelain() {
        let worktrees = parse_worktrees(
            "project-1",
            "worktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo-feature\nHEAD def456\ndetached\nlocked reason\nprunable stale\n\n",
        );
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, "/repo");
        assert_eq!(worktrees[0].branch, "main");
        assert!(worktrees[0].is_main);
        assert!(worktrees[1].detached);
        assert!(worktrees[1].locked);
        assert!(worktrees[1].prunable);
    }

    #[test]
    fn registered_projects_require_a_notes_path_and_default_worktree_colors() {
        let missing_notes = serde_json::from_value::<StoredProject>(serde_json::json!({
            "id": "project-1",
            "name": "Project",
            "registered_path": "/repo",
            "common_git_dir": "/repo/.git"
        }));
        assert!(missing_notes.is_err());

        let project: StoredProject = serde_json::from_value(serde_json::json!({
            "id": "project-1",
            "name": "Project",
            "registered_path": "/repo",
            "common_git_dir": "/repo/.git",
            "notes_path": "/notes/project-1"
        }))
        .unwrap();
        assert!(project.worktree_colors.is_empty());
        assert!(project.worktree_icons.is_empty());
        assert!(
            serde_json::from_value::<SetWorktreeIconRequest>(serde_json::json!({
                "path": "/repo", "icon": "unknown"
            }))
            .is_err()
        );
    }

    #[test]
    fn maps_supported_remote_urls_to_the_same_notes_identity() {
        let expected = PathBuf::from("github.com/openai/codex");
        assert_eq!(
            parse_remote_identity("git@github.com:openai/codex.git").unwrap(),
            expected
        );
        assert_eq!(
            parse_remote_identity("https://github.com/openai/codex.git").unwrap(),
            expected
        );
        assert_eq!(
            parse_remote_identity("ssh://git@github.com/openai/codex.git").unwrap(),
            expected
        );
        assert_eq!(
            parse_remote_identity("https://git.example.com/group/subgroup/repo.git").unwrap(),
            PathBuf::from("git.example.com/group/subgroup/repo")
        );
    }

    #[test]
    fn rejects_unsafe_remote_paths() {
        assert!(parse_remote_identity("file:///tmp/repo").is_err());
        assert!(parse_remote_identity("git@example.com:../repo.git").is_err());
    }

    #[test]
    fn maps_local_repository_names_to_localhost_notes_identities() {
        assert_eq!(
            local_repository_identity(Path::new("/projects/example"), "alice").unwrap(),
            PathBuf::from("localhost/alice/example")
        );
        assert!(local_repository_identity(Path::new("/projects/example"), "../alice").is_err());
    }

    #[tokio::test]
    async fn pinned_directories_persist_merge_and_preserve_other_settings() {
        let directory = tempfile::tempdir().unwrap();
        let notes_base = directory.path().join("notes");
        std::fs::write(directory.path().join(SETTINGS_FILE), serde_json::to_vec(&serde_json::json!({
            "version": 1, "notes_base": notes_base, "pinned_worktrees": ["/repo/main"], "pinned_worktrees_revision": 7,
        })).unwrap()).unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        assert!(manager.pinned_directories().unwrap().paths.is_empty());
        let first = directory.path().join("目录 #1");
        let second = directory.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let first = first.to_string_lossy().into_owned();
        let second = second.to_string_lossy().into_owned();
        let (one, two) = tokio::join!(
            manager.update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec![first.clone(), format!("{first}/")],
                ..Default::default()
            }),
            manager.update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec![second.clone()],
                ..Default::default()
            }),
        );
        one.unwrap();
        two.unwrap();
        let pins = manager.pinned_directories().unwrap();
        assert_eq!(pins.paths.len(), 2);
        assert!(pins.paths.contains(&first));
        assert!(pins.paths.contains(&second));
        assert_eq!(pins.revision, 2);
        let duplicate = manager
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec![first.clone()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(duplicate.revision, pins.revision);
        let configured_notes = directory.path().join("configured-notes");
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(configured_notes.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .await
            .unwrap();
        manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/feature".into()],
                ..Default::default()
            })
            .unwrap();
        drop(manager);
        let reloaded = AowManager::persistent(directory.path()).unwrap();
        assert_eq!(reloaded.pinned_directories().unwrap().paths, pins.paths);
        assert_eq!(
            reloaded.pinned_directories().unwrap().revision,
            pins.revision
        );
        assert_eq!(
            reloaded.pinned_worktrees().unwrap().paths,
            ["/repo/main", "/repo/feature"]
        );
        assert_eq!(
            Path::new(&reloaded.settings().unwrap().notes_base),
            configured_notes
        );
        std::fs::remove_dir(&first).unwrap();
        let remaining = reloaded
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                remove: vec![first],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(remaining.paths, [second]);
        assert_eq!(remaining.revision, 3);
        drop(reloaded);
        assert_eq!(
            AowManager::persistent(directory.path())
                .unwrap()
                .pinned_directories()
                .unwrap()
                .paths,
            remaining.paths
        );
    }

    #[tokio::test]
    async fn pinned_directories_roll_back_when_config_write_fails() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        let previous = manager
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec!["/".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        let settings_path = manager.inner.settings_path.as_ref().unwrap();
        std::fs::remove_file(&settings_path).unwrap();
        std::fs::create_dir(&settings_path).unwrap();
        assert!(
            manager
                .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                    remove: vec!["/".into()],
                    ..Default::default()
                })
                .await
                .is_err()
        );
        let unchanged = manager.pinned_directories().unwrap();
        assert_eq!(unchanged.paths, previous.paths);
        assert_eq!(unchanged.revision, previous.revision);
    }

    #[tokio::test]
    async fn pinned_directories_routes_validate_paths_and_reject_files() {
        use axum::{
            body::{Body, to_bytes},
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let regular_file = directory.path().join("file.txt");
        std::fs::write(&regular_file, "not a directory").unwrap();
        let app = crate::build_router(AppState::new(directory.path().to_path_buf()));
        for invalid in [
            "relative/path",
            "/bad\0path",
            regular_file.to_str().unwrap(),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::patch("/api/aow/pinned-directories")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({ "add": [invalid] }).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        for (request, expected) in [
            (
                serde_json::json!({ "add": ["/"] }),
                serde_json::json!(["/"]),
            ),
            (
                serde_json::json!({ "remove": ["/"] }),
                serde_json::json!([]),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::patch("/api/aow/pinned-directories")
                        .header("content-type", "application/json")
                        .body(Body::from(request.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let response = app
                .clone()
                .oneshot(
                    Request::get("/api/aow/pinned-directories")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let payload: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(payload["paths"], expected);
        }
    }

    #[tokio::test]
    async fn pinned_worktrees_persist_and_merge_without_losing_other_settings() {
        let directory = tempfile::tempdir().unwrap();
        let notes_base = directory.path().join("notes");
        let settings_path = directory.path().join(SETTINGS_FILE);
        std::fs::write(
            &settings_path,
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "notes_base": notes_base,
            }))
            .unwrap(),
        )
        .unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        assert!(manager.pinned_worktrees().unwrap().paths.is_empty());
        let pins = manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/b".into(), "/repo/a".into(), "/repo/b".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(pins.paths, ["/repo/b", "/repo/a"]);
        manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/c".into()],
                ..Default::default()
            })
            .unwrap();
        let pins = manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                order: Some(vec![
                    "/repo/a".into(),
                    "/repo/b".into(),
                    "/repo/missing".into(),
                ]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(pins.paths, ["/repo/a", "/repo/b", "/repo/c"]);
        let pins = manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                remove: vec!["/repo/b".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(pins.paths, ["/repo/a", "/repo/c"]);
        let configured_notes = directory.path().join("configured-notes");
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(configured_notes.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .await
            .unwrap();
        drop(manager);
        let reloaded = AowManager::persistent(directory.path()).unwrap();
        assert_eq!(reloaded.pinned_worktrees().unwrap().paths, pins.paths);
        assert_eq!(reloaded.pinned_worktrees().unwrap().revision, pins.revision);
        assert_eq!(
            Path::new(&reloaded.settings().unwrap().notes_base),
            configured_notes
        );
        assert!(
            reloaded
                .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                    add: vec!["relative/path".into()],
                    ..Default::default()
                })
                .is_err()
        );
    }

    #[test]
    fn pinned_worktrees_roll_back_when_config_write_fails() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        let previous = manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/a".into()],
                ..Default::default()
            })
            .unwrap();
        let settings_path = manager.inner.settings_path.as_ref().unwrap();
        std::fs::remove_file(&settings_path).unwrap();
        std::fs::create_dir(&settings_path).unwrap();
        assert!(
            manager
                .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                    add: vec!["/repo/b".into()],
                    ..Default::default()
                })
                .is_err()
        );
        let unchanged = manager.pinned_worktrees().unwrap();
        assert_eq!(unchanged.paths, previous.paths);
        assert_eq!(unchanged.revision, previous.revision);
    }

    #[tokio::test]
    async fn persists_configured_notes_base_and_uses_it_for_new_projects() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            StdCommand::new("git")
                .args(["remote", "add", "origin", "git@github.com:openai/codex.git"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        let state_directory = directory.path().join("state");
        let notes_base = directory.path().join("configured-notes");
        let manager = AowManager::persistent_with_notes_base(
            &state_directory,
            directory.path().join("default-notes"),
        )
        .unwrap();

        let settings = manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(notes_base.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(Path::new(&settings.notes_base), notes_base);
        assert!(notes_base.is_dir());

        let project = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        assert_eq!(
            Path::new(&project.notes_path),
            notes_base.join("github.com/openai/codex")
        );

        drop(manager);
        let reloaded = AowManager::persistent_with_notes_base(
            &state_directory,
            directory.path().join("another-default"),
        )
        .unwrap();
        assert_eq!(
            Path::new(&reloaded.settings().unwrap().notes_base),
            notes_base
        );
    }

    #[tokio::test]
    async fn allows_notes_base_nested_in_a_git_repository() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repository");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        let nested = repository.join("notes");
        let result = prepare_notes_base_directory(&nested).await.unwrap();
        assert_eq!(result, nested);
        assert!(nested.is_dir());
    }

    #[test]
    fn generates_short_note_ids_from_the_safe_alphabet() {
        let first = short_random_id();
        let second = short_random_id();
        assert_eq!(first.len(), 8);
        assert_eq!(second.len(), 8);
        assert_ne!(first, second);
        assert!(
            first
                .chars()
                .all(|character| { "23456789abcdefghjkmnpqrstuvwxyz".contains(character) })
        );
    }

    #[tokio::test]
    async fn retries_a_colliding_temporary_note_name_without_scanning() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".tmp-aaaaaaaa.md"), b"existing").unwrap();
        let mut ids = ["aaaaaaaa", "bbbbbbbb"].into_iter();

        let (path, name) =
            create_temporary_note_file(directory.path(), "md", || ids.next().unwrap().into())
                .await
                .unwrap();

        assert_eq!(name, ".tmp-bbbbbbbb.md");
        assert_eq!(path, directory.path().join(&name));
        assert_eq!(
            std::fs::read(directory.path().join(".tmp-aaaaaaaa.md")).unwrap(),
            b"existing"
        );
    }

    #[tokio::test]
    async fn registers_and_reuses_a_remote_mapped_notes_directory() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            StdCommand::new("git")
                .args(["remote", "add", "origin", "git@github.com:openai/codex.git"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        let manager = AowManager::persistent_with_notes_base(
            &directory.path().join("state"),
            directory.path().join("aow"),
        )
        .unwrap();
        let project = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        let expected = directory.path().join("aow/github.com/openai/codex");
        assert_eq!(Path::new(&project.notes_path), expected);
        assert!(expected.is_dir());
        assert!(!expected.join(".git").exists());
        std::fs::write(expected.join("kept.md"), b"keep me").unwrap();

        let reregistered = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        assert_eq!(reregistered.notes_path, project.notes_path);
        assert_eq!(std::fs::read(expected.join("kept.md")).unwrap(), b"keep me");
    }

    #[tokio::test]
    async fn registers_a_local_repository_without_a_remote_under_localhost() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        let manager = AowManager::persistent_with_notes_base(
            &directory.path().join("state"),
            directory.path().join("aow"),
        )
        .unwrap();

        let project = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();

        let expected = directory
            .path()
            .join("aow")
            .join("localhost")
            .join(process_account_name().unwrap())
            .join("repo");
        assert_eq!(Path::new(&project.notes_path), expected);
        assert!(expected.is_dir());
        assert!(!expected.join(".git").exists());
    }

    #[tokio::test]
    async fn allows_notes_directories_nested_in_another_repository() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("outer");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        let nested = repository.join("notes");
        let result = prepare_notes_directory(&nested).await.unwrap();
        assert_eq!(Path::new(&result), nested);
        assert!(nested.is_dir());
        assert!(!nested.join(".git").exists());
    }

    #[test]
    fn executable_resolution_does_not_invoke_the_command() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("agent");
        std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            resolve_executable("agent", &[directory.path().to_path_buf()]),
            Some(executable)
        );
    }

    #[test]
    fn agent_discovery_only_registers_the_traecli_command() {
        let directory = tempfile::tempdir().unwrap();
        let paths = [directory.path().to_path_buf()];
        let manager = AowManager::in_memory();
        for command in ["traex", "traecli"] {
            let executable = directory.path().join(command);
            std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let agents = manager.agents_in_path(&paths).unwrap();
            if command == "traex" {
                assert!(agents.is_empty());
            } else {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].id, "traecli");
                assert_eq!(agents[0].display_name, "TraeCode CLI");
                assert_eq!(agents[0].source, "detected");
                assert_eq!(agents[0].executable.as_deref(), executable.to_str());
            }
        }
    }

    #[test]
    fn agent_discovery_is_limited_to_supported_registration_types() {
        let directory = tempfile::tempdir().unwrap();
        for agent in aow_agents::KNOWN_AGENTS {
            for command in agent.definition().commands {
                let executable = directory.path().join(command);
                std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
                std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
        }
        let agents = AowManager::in_memory()
            .agents_in_path(&[directory.path().to_path_buf()])
            .unwrap();
        assert_eq!(agents.len(), AgentType::ALL.len());
        for agent_type in AgentType::ALL {
            assert!(agents.iter().any(|agent| {
                agent.id == agent_type.id() && agent.agent_type == Some(agent_type)
            }));
        }
    }

    #[tokio::test]
    async fn agent_registration_api_requires_a_supported_type() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let app = crate::build_router(AppState::new(directory.path().to_path_buf()));
        for agent_type in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!("")),
            Some(serde_json::json!("gemini")),
            Some(serde_json::json!("custom")),
        ] {
            let mut request =
                serde_json::json!({"display_name": "Custom wrapper", "command": "/bin/sh"});
            if let Some(agent_type) = agent_type {
                request["agent_type"] = agent_type;
            }
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/aow/agents")
                        .header("content-type", "application/json")
                        .body(Body::from(request.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                matches!(
                    response.status(),
                    StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
                ),
                "{request}: {}",
                response.status()
            );
        }
    }

    #[tokio::test]
    async fn agent_types_persist_for_multiple_custom_launch_configurations() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        manager
            .update_settings(UpdateSettingsRequest {
                execution_path: Some(vec![directory.path().to_path_buf()]),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut registrations = Vec::new();
        for agent_type in [
            AgentType::Claude,
            AgentType::Codex,
            AgentType::TraeCli,
            AgentType::Codex,
        ] {
            let registration = manager.register_agent(serde_json::from_value(serde_json::json!({
                "agent_type": agent_type, "display_name": "My wrapper", "command": "/bin/sh",
                "args": ["-c", "printf configured"], "env": {"CUSTOM_ENDPOINT": "value with spaces", "EMPTY": ""}
            })).unwrap()).await.unwrap();
            assert_eq!(registration.agent_type, Some(agent_type));
            assert!(registration.available);
            registrations.push(registration);
        }
        let manager = AowManager::persistent(directory.path()).unwrap();
        let agents = manager.agents().await.unwrap();
        assert_eq!(agents.len(), 4);
        assert_eq!(
            agents
                .iter()
                .map(|agent| &agent.id)
                .collect::<HashSet<_>>()
                .len(),
            4
        );
        for registration in &registrations {
            let saved = agents
                .iter()
                .find(|agent| agent.id == registration.id)
                .unwrap();
            assert_eq!(saved.agent_type, registration.agent_type);
            let launch = saved
                .clone()
                .into_launch(&[directory.path().to_path_buf()])
                .unwrap();
            assert_eq!(Some(launch.agent_type), registration.agent_type);
            assert_eq!(launch.executable, "/bin/sh");
            assert_eq!(launch.args, ["-c", "printf configured"]);
            assert_eq!(launch.env["CUSTOM_ENDPOINT"], "value with spaces");
            assert_eq!(launch.env["EMPTY"], "");
        }
        let updated = manager.register_agent(serde_json::from_value(serde_json::json!({
            "id": registrations[0].id, "agent_type": "traecli", "display_name": "Another wrapper",
            "command": "/bin/sh", "args": ["--custom"]
        })).unwrap()).await.unwrap();
        assert_eq!(updated.agent_type, Some(AgentType::TraeCli));
        assert_eq!(manager.agents().await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn legacy_agent_configuration_requires_type_only_when_identity_is_unknown() {
        let manager = AowManager::in_memory();
        for id in ["codex", "custom-wrapper"] {
            manager.lock().unwrap().agents.push(serde_json::from_value(serde_json::json!({
                "id": id, "display_name": id, "command": "/bin/sh", "args": ["--keep"], "env": {"KEEP": "value"}
            })).unwrap());
        }
        let agents = manager.agents_in_path(&[]).unwrap();
        let builtin = agents.iter().find(|agent| agent.id == "codex").unwrap();
        assert_eq!(builtin.agent_type, Some(AgentType::Codex));
        assert!(builtin.available);
        let legacy = agents
            .iter()
            .find(|agent| agent.id == "custom-wrapper")
            .unwrap();
        assert_eq!(legacy.agent_type, None);
        assert!(!legacy.available);
        assert_eq!(legacy.command, "/bin/sh");
        assert_eq!(legacy.args, ["--keep"]);
        assert_eq!(legacy.env["KEEP"], "value");
        assert!(matches!(
            legacy.clone().into_launch(&[]),
            Err(aow_agents::launch::LaunchError::Invalid(_))
        ));
        let mut update = serde_json::to_value(legacy).unwrap();
        update["agent_type"] = serde_json::json!("claude");
        let saved = manager
            .register_agent(serde_json::from_value(update).unwrap())
            .await
            .unwrap();
        assert_eq!(saved.id, "custom-wrapper");
        assert_eq!(saved.agent_type, Some(AgentType::Claude));
        assert_eq!(saved.args, legacy.args);
        assert_eq!(saved.env, legacy.env);
        assert!(saved.available);
        let mismatch = manager.register_agent(serde_json::from_value(serde_json::json!({
            "id": "codex", "agent_type": "claude", "display_name": "Mismatch", "command": "/bin/sh"
        })).unwrap()).await;
        assert!(matches!(mismatch, Err(AowError::Invalid(_))));
        assert_eq!(
            manager
                .agents_in_path(&[])
                .unwrap()
                .iter()
                .find(|agent| agent.id == "codex")
                .unwrap()
                .agent_type,
            Some(AgentType::Codex)
        );
    }

    #[tokio::test]
    async fn detected_agent_configuration_persists_launches_and_resets() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let executable = bin.join("codex");
        std::fs::write(
            &executable,
            b"#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" \"$AOW_AGENT_TEST\" \"$EMPTY\" \"$HOME\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        manager
            .update_settings(UpdateSettingsRequest {
                execution_path: Some(vec![bin.clone()]),
                ..Default::default()
            })
            .await
            .unwrap();
        let detected = manager.agents().await.unwrap().pop().unwrap();
        assert_eq!(detected.id, "codex");
        assert_eq!(detected.source, "detected");
        assert_eq!(detected.command, "codex");
        let request = serde_json::json!({
            "id": detected.id,
            "agent_type": "codex",
            "display_name": detected.display_name,
            "command": detected.command,
            "args": ["--model", "model with spaces"],
            "env": {"AOW_AGENT_TEST": "value with spaces=equals", "EMPTY": "", "HOME": "/agent/home"}
        });
        manager
            .register_agent(serde_json::from_value(request.clone()).unwrap())
            .await
            .unwrap();

        let manager = AowManager::persistent(directory.path()).unwrap();
        let agents = manager.agents().await.unwrap();
        assert_eq!(
            agents.len(),
            1,
            "overrides must not duplicate detected agents"
        );
        let configured = &agents[0];
        assert_eq!(configured.id, "codex");
        assert_eq!(configured.source, "configured");
        assert_eq!(configured.command, "codex");
        assert_eq!(configured.args, ["--model", "model with spaces"]);
        assert_eq!(configured.env["EMPTY"], "");
        assert_eq!(
            std::fs::metadata(manager.inner.agents_path.as_ref().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let launch = agents
            .into_iter()
            .next()
            .unwrap()
            .into_launch(&[bin.clone()])
            .unwrap();
        let output = StdCommand::new(&launch.executable)
            .args(&launch.args)
            .env_clear()
            .envs(&launch.env)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "--model\nmodel with spaces\nvalue with spaces=equals\n\n/agent/home\n"
        );

        let mut update = request;
        update["args"] = serde_json::json!([]);
        update["env"] = serde_json::json!({});
        manager
            .register_agent(serde_json::from_value(update).unwrap())
            .await
            .unwrap();
        let updated = manager.agents().await.unwrap();
        assert_eq!(updated.len(), 1);
        assert!(updated[0].args.is_empty());
        assert!(updated[0].env.is_empty());
        manager.remove_agent("codex").unwrap();
        let restored = AowManager::persistent(directory.path())
            .unwrap()
            .agents()
            .await
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].source, "detected");
        assert!(restored[0].args.is_empty());
        assert!(restored[0].env.is_empty());
    }

    #[tokio::test]
    async fn agent_configuration_rejects_invalid_environment_and_failed_writes() {
        let directory = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent(directory.path()).unwrap();
        let request = serde_json::json!({
            "id": "custom-test", "agent_type": "codex", "display_name": "Test", "command": "/bin/sh", "env_keys": ["HOME"]
        });
        let legacy: StoredAgent = serde_json::from_value(request.clone()).unwrap();
        assert!(legacy.env.is_empty());
        assert!(
            serde_json::to_value(&legacy)
                .unwrap()
                .get("env_keys")
                .is_none()
        );
        for env in [
            serde_json::json!({"BAD=KEY": "value"}),
            serde_json::json!({"KEY": "bad\0value"}),
        ] {
            let mut invalid = request.clone();
            invalid["env"] = env;
            assert!(matches!(
                manager
                    .register_agent(serde_json::from_value(invalid).unwrap())
                    .await,
                Err(AowError::Invalid(_))
            ));
            assert!(manager.lock().unwrap().agents.is_empty());
        }
        manager
            .register_agent(serde_json::from_value(request.clone()).unwrap())
            .await
            .unwrap();
        let registry = manager.inner.agents_path.as_ref().unwrap();
        std::fs::remove_file(&registry).unwrap();
        std::fs::create_dir(&registry).unwrap();
        let mut update = request.clone();
        update["args"] = serde_json::json!(["--changed"]);
        assert!(
            manager
                .register_agent(serde_json::from_value(update).unwrap())
                .await
                .is_err()
        );
        assert!(manager.lock().unwrap().agents[0].args.is_empty());
        let mut create = request;
        create["id"] = serde_json::json!("another-agent");
        assert!(
            manager
                .register_agent(serde_json::from_value(create).unwrap())
                .await
                .is_err()
        );
        assert_eq!(manager.lock().unwrap().agents.len(), 1);
    }

    #[test]
    fn agent_launch_resolves_env_shebang_with_discovered_path() {
        let directory = tempfile::tempdir().unwrap();
        let cli_bin = directory.path().join("cli bin");
        let node_bin = directory.path().join("node bin");
        std::fs::create_dir(&cli_bin).unwrap();
        std::fs::create_dir(&node_bin).unwrap();
        let executable = cli_bin.join("codex");
        let node = node_bin.join("node");
        std::fs::write(&executable, b"#!/usr/bin/env node\n").unwrap();
        // A stand-in interpreter keeps the regression independent of installed Node.
        std::fs::write(&node, b"#!/bin/sh\nprintf 'node-ready:%s\\n' \"$2\"\n").unwrap();
        for path in [&executable, &node] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let paths = vec![cli_bin.clone(), node_bin];
        let manager = AowManager::in_memory();

        // The CLI itself can be found even when its interpreter cannot.
        let missing_node = StdCommand::new(&executable)
            .env_clear()
            .env("PATH", &cli_bin)
            .output()
            .unwrap();
        assert_eq!(missing_node.status.code(), Some(127));

        for configured in [false, true] {
            if configured {
                manager.lock().unwrap().agents.push(StoredAgent {
                    agent_type: Some(AgentType::Codex),
                    id: "codex".to_owned(),
                    display_name: "Configured Codex".to_owned(),
                    command: executable.to_string_lossy().into_owned(),
                    args: vec!["--version".to_owned()],
                    env: BTreeMap::new(),
                });
            }
            let agent = manager
                .agents_in_path(&paths)
                .unwrap()
                .into_iter()
                .find(|agent| agent.id == "codex" && agent.available)
                .unwrap();
            assert_eq!(
                agent.source,
                if configured { "configured" } else { "detected" }
            );
            let launch = agent.into_launch(&paths).unwrap();
            let output = StdCommand::new(&launch.executable)
                .args(&launch.args)
                .env_clear()
                .env("PATH", &cli_bin)
                .envs(&launch.env)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "agent failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                if configured {
                    "node-ready:--version\n"
                } else {
                    "node-ready:\n"
                }
            );
        }
    }

    #[test]
    fn agent_launch_adds_configured_environment_with_global_path() {
        let agent = AgentRegistration {
            id: "custom".to_owned(),
            agent_type: Some(AgentType::Codex),
            display_name: "Custom Agent".to_owned(),
            source: "configured",
            available: true,
            command: "/bin/sh".to_owned(),
            executable: Some("/bin/sh".to_owned()),
            args: Vec::new(),
            env: BTreeMap::from([
                ("PATH".to_owned(), "/agent/bin".to_owned()),
                ("AOW_AGENT_TEST".to_owned(), "value".to_owned()),
                ("EMPTY".to_owned(), String::new()),
            ]),
        };
        let paths = vec![PathBuf::from("/discovered/bin")];
        let launch = agent.into_launch(&paths).unwrap();
        assert_eq!(launch.env["PATH"], "/discovered/bin");
        assert_eq!(launch.env["AOW_AGENT_TEST"], "value");
        assert_eq!(launch.env["EMPTY"], "");
        assert_eq!(launch.env.len(), 3);
    }

    fn test_agent_session(
        agent: &'static str,
        session_id: &str,
        cwd: &Path,
        transcript_path: &Path,
        trusted_root: &Path,
    ) -> aow_agents::sessions::AgentSession {
        aow_agents::sessions::AgentSession::new(
            aow_agents::sessions::AgentSessionLocator {
                agent,
                session_id: session_id.into(),
                title: "Test session".into(),
                cwd: cwd.into(),
                transcript_path: transcript_path.into(),
                trusted_root: trusted_root.into(),
            },
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
    }

    #[test]
    fn session_locator_is_scoped_to_the_exact_registered_worktree() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("repo");
        let nested = parent.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let transcript = directory.path().join("sessions/session.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, b"").unwrap();
        let manager = AowManager::in_memory();
        let sessions = vec![
            test_agent_session(
                "traecli",
                "session-1",
                &nested,
                &transcript,
                transcript.parent().unwrap(),
            ),
            test_agent_session(
                "codex",
                "session-2",
                &nested,
                &transcript,
                transcript.parent().unwrap(),
            ),
        ];

        manager
            .replace_session_locators_for_agent(&parent, None, &sessions)
            .unwrap();
        assert!(
            manager
                .session_locator(&parent, "traecli", "session-1")
                .is_ok()
        );
        assert!(
            manager
                .session_locator(&nested, "traecli", "session-1")
                .is_err()
        );
        assert!(
            manager
                .session_locator(&parent, "codex", "session-1")
                .is_err()
        );

        manager
            .replace_session_locators_for_agent(&parent, Some("traecli"), &[])
            .unwrap();
        assert!(
            manager
                .session_locator(&parent, "traecli", "session-1")
                .is_err()
        );
        assert!(
            manager
                .session_locator(&parent, "codex", "session-2")
                .is_ok()
        );
    }

    #[test]
    fn removed_session_worktree_paths_must_stay_absolute_and_normalized() {
        assert_eq!(
            removed_directory_path(Path::new("/tmp/removed-worktree")).unwrap(),
            Path::new("/tmp/removed-worktree")
        );
        assert!(removed_directory_path(Path::new("relative/worktree")).is_err());
        assert!(removed_directory_path(Path::new("/tmp/../removed-worktree")).is_err());
    }

    #[tokio::test]
    async fn optionally_pulls_main_repository_when_registered_from_linked_worktree() {
        let directory = tempfile::tempdir().unwrap();
        let origin = directory.path().join("origin");
        let repository = directory.path().join("repo");
        let linked = directory.path().join("repo-linked");
        git_output(
            directory.path(),
            &["init", "-q", "-b", "main", origin.to_str().unwrap()],
        )
        .await
        .unwrap();
        let commit_args = [
            "-c",
            "user.name=AoW Test",
            "-c",
            "user.email=aow@example.com",
            "commit",
            "-q",
            "-m",
            "update version",
        ];
        std::fs::write(origin.join("version.txt"), "initial\n").unwrap();
        git_output(&origin, &["add", "version.txt"]).await.unwrap();
        git_output(&origin, &commit_args).await.unwrap();
        git_output(
            directory.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                repository.to_str().unwrap(),
            ],
        )
        .await
        .unwrap();
        git_output(
            &repository,
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        )
        .await
        .unwrap();
        let initial_head = git_output(&repository, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(origin.join("version.txt"), "latest\n").unwrap();
        git_output(&origin, &["add", "version.txt"]).await.unwrap();
        git_output(&origin, &commit_args).await.unwrap();
        let latest_head = git_output(&origin, &["rev-parse", "HEAD"]).await.unwrap();
        assert_ne!(initial_head, latest_head);

        let manager = AowManager::in_memory();
        let project = manager
            .register_project(RegisterProjectRequest {
                path: linked.to_string_lossy().into_owned(),
                name: None,
                notes_path: Some(
                    directory
                        .path()
                        .join("notes")
                        .to_string_lossy()
                        .into_owned(),
                ),
            })
            .await
            .unwrap();

        for pull_first in [false, true] {
            let worktree_path = directory.path().join(format!("repo-pull-{pull_first}"));
            let result = manager
                .create_worktree(
                    &project.id,
                    CreateWorktreeRequest {
                        branch: format!("feature/pull-{pull_first}"),
                        base_ref: "main".to_owned(),
                        path: worktree_path.to_string_lossy().into_owned(),
                        pull_first,
                    },
                )
                .await
                .unwrap();
            let expected_head = if pull_first {
                &latest_head
            } else {
                &initial_head
            };
            assert_eq!(result.worktree.head, expected_head.trim());
            assert_eq!(
                git_output(&repository, &["rev-parse", "HEAD"])
                    .await
                    .unwrap(),
                *expected_head
            );
            assert_eq!(
                std::fs::read_to_string(worktree_path.join("version.txt")).unwrap(),
                if pull_first { "latest\n" } else { "initial\n" }
            );
            assert_eq!(
                git_output(&linked, &["rev-parse", "HEAD"]).await.unwrap(),
                initial_head
            );
        }
    }

    #[tokio::test]
    async fn creates_worktree_and_returns_refreshed_project() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        assert!(
            StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            StdCommand::new("git")
                .args([
                    "-c",
                    "user.name=AoW Test",
                    "-c",
                    "user.email=aow@example.com",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "initial",
                ])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );

        let state_directory = directory.path().join("state");
        let manager = AowManager::persistent(&state_directory).unwrap();
        let project = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: Some(
                    directory
                        .path()
                        .join("notes")
                        .to_string_lossy()
                        .into_owned(),
                ),
            })
            .await
            .unwrap();
        let worktree_path = directory.path().join("repo-feature");
        let failed_pull = manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: "feature/aow".to_owned(),
                    base_ref: "main".to_owned(),
                    path: worktree_path.to_string_lossy().into_owned(),
                    pull_first: true,
                },
            )
            .await
            .unwrap_err();
        assert!(
            failed_pull
                .to_string()
                .contains("git pull failed in main worktree")
        );
        assert!(!worktree_path.exists());
        assert!(
            git_output(&repository, &["branch", "--list", "feature/aow"])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            manager.project(&project.id).await.unwrap().worktrees.len(),
            1
        );

        let result = manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: "feature/aow".to_owned(),
                    base_ref: "main".to_owned(),
                    path: worktree_path.to_string_lossy().into_owned(),
                    pull_first: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(result.worktree.branch, "feature/aow");
        assert_eq!(Path::new(&result.worktree.path), worktree_path);
        assert_eq!(
            manager
                .project_name_for_workspace(repository.to_str().unwrap())
                .await,
            Some(project.name.clone())
        );
        assert_eq!(
            manager
                .project_name_for_workspace(worktree_path.to_str().unwrap())
                .await,
            Some(project.name.clone())
        );
        assert_eq!(result.project.worktrees.len(), 2);
        assert!(
            result
                .project
                .worktrees
                .iter()
                .any(|worktree| worktree.id == result.worktree.id)
        );
        assert_eq!(result.worktree.color, WorktreeColor::Default);
        assert_eq!(result.worktree.icon, WorktreeIcon::Default);

        let icon_request = |icon| SetWorktreeIconRequest {
            path: worktree_path.to_string_lossy().into_owned(),
            icon,
        };
        manager
            .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Cat))
            .await
            .unwrap();

        let colored = manager
            .set_worktree_color(
                &project.id,
                SetWorktreeColorRequest {
                    path: worktree_path.to_string_lossy().into_owned(),
                    color: WorktreeColor::Purple,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            colored
                .worktrees
                .iter()
                .find(|w| Path::new(&w.path) == worktree_path)
                .unwrap()
                .icon,
            WorktreeIcon::Cat
        );
        assert_eq!(
            colored
                .worktrees
                .iter()
                .find(|worktree| Path::new(&worktree.path) == worktree_path)
                .unwrap()
                .color,
            WorktreeColor::Purple
        );
        drop(manager);
        let manager = AowManager::persistent(&state_directory).unwrap();
        let reloaded = manager.project(&project.id).await.unwrap();
        assert_eq!(
            reloaded
                .worktrees
                .iter()
                .find(|w| Path::new(&w.path) == worktree_path)
                .unwrap()
                .icon,
            WorktreeIcon::Cat
        );
        assert_eq!(
            reloaded
                .worktrees
                .iter()
                .find(|worktree| Path::new(&worktree.path) == worktree_path)
                .unwrap()
                .color,
            WorktreeColor::Purple
        );

        // Changing or resetting the shape preserves the independently selected color.
        for icon in [
            WorktreeIcon::Pear,
            WorktreeIcon::Default,
            WorktreeIcon::Flower2,
        ] {
            let updated = manager
                .set_worktree_icon(&project.id, icon_request(icon))
                .await
                .unwrap();
            let worktree = updated
                .worktrees
                .iter()
                .find(|w| Path::new(&w.path) == worktree_path)
                .unwrap();
            assert_eq!(worktree.icon, icon);
            assert_eq!(worktree.color, WorktreeColor::Purple);
            if icon == WorktreeIcon::Default {
                assert!(
                    !manager
                        .lock()
                        .unwrap()
                        .projects
                        .iter()
                        .find(|p| p.id == project.id)
                        .unwrap()
                        .worktree_icons
                        .contains_key(worktree_path.to_str().unwrap())
                );
            }
        }
        assert!(matches!(
            manager
                .set_worktree_icon(
                    &project.id,
                    SetWorktreeIconRequest {
                        path: directory
                            .path()
                            .join("not-a-worktree")
                            .to_string_lossy()
                            .into_owned(),
                        icon: WorktreeIcon::Cat,
                    }
                )
                .await,
            Err(AowError::Invalid(_))
        ));

        // A failed disk write must restore the in-memory shape as well.
        let projects_path = manager.inner.projects_path.as_ref().unwrap();
        let saved_projects = std::fs::read(projects_path).unwrap();
        std::fs::remove_file(projects_path).unwrap();
        std::fs::create_dir(projects_path).unwrap();
        assert!(
            manager
                .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Dog))
                .await
                .is_err()
        );
        let unchanged = manager.project(&project.id).await.unwrap();
        let worktree = unchanged
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == worktree_path)
            .unwrap();
        assert_eq!(worktree.icon, WorktreeIcon::Flower2);
        assert_eq!(worktree.color, WorktreeColor::Purple);
        std::fs::remove_dir(projects_path).unwrap();
        std::fs::write(projects_path, saved_projects).unwrap();

        let duplicate_path = directory.path().join("repo-duplicate");
        let duplicate = manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: "feature/aow".to_owned(),
                    base_ref: "main".to_owned(),
                    path: duplicate_path.to_string_lossy().into_owned(),
                    pull_first: false,
                },
            )
            .await;
        assert!(matches!(duplicate, Err(AowError::Git(_))));
        assert!(!duplicate_path.exists());

        let existing_path = directory.path().join("already-exists");
        std::fs::create_dir(&existing_path).unwrap();
        let existing = manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: "feature/other".to_owned(),
                    base_ref: "main".to_owned(),
                    path: existing_path.to_string_lossy().into_owned(),
                    pull_first: false,
                },
            )
            .await;
        assert!(matches!(existing, Err(AowError::Invalid(_))));

        std::fs::write(worktree_path.join("uncommitted.txt"), b"not committed\n").unwrap();
        let inspection = manager
            .inspect_worktree_removal(&project.id, worktree_path.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(inspection.changes, vec!["?? uncommitted.txt"]);
        assert_eq!(inspection.change_count, 1);
        assert!(!inspection.truncated);

        let without_force = manager
            .remove_worktree(&project.id, worktree_path.to_str().unwrap(), false)
            .await;
        assert!(matches!(without_force, Err(AowError::DirtyWorktree(1))));
        assert!(worktree_path.exists());

        let reregistered = manager
            .register_project(RegisterProjectRequest {
                path: worktree_path.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        assert_eq!(reregistered.id, project.id);
        assert_eq!(Path::new(&reregistered.registered_path), worktree_path);

        let refreshed = manager
            .remove_worktree(&project.id, worktree_path.to_str().unwrap(), true)
            .await
            .unwrap();
        assert!(!worktree_path.exists());
        let document: RegistryDocument<StoredProject> = serde_json::from_slice(
            &std::fs::read(manager.inner.projects_path.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(
            !document.items[0]
                .worktree_colors
                .contains_key(worktree_path.to_str().unwrap())
        );
        assert!(
            !document.items[0]
                .worktree_icons
                .contains_key(worktree_path.to_str().unwrap())
        );
        assert_eq!(Path::new(&refreshed.registered_path), repository);
        assert_eq!(refreshed.worktrees.len(), 1);
        assert!(refreshed.worktrees[0].is_main);
        let branches = git_output(&repository, &["branch", "--format=%(refname:short)"])
            .await
            .unwrap();
        assert!(branches.lines().any(|branch| branch == "feature/aow"));

        let clean_path = directory.path().join("repo-clean");
        manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: "feature/clean".to_owned(),
                    base_ref: "main".to_owned(),
                    path: clean_path.to_string_lossy().into_owned(),
                    pull_first: false,
                },
            )
            .await
            .unwrap();
        let clean_inspection = manager
            .inspect_worktree_removal(&project.id, clean_path.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(clean_inspection.change_count, 0);
        manager
            .remove_worktree(&project.id, clean_path.to_str().unwrap(), false)
            .await
            .unwrap();
        assert!(!clean_path.exists());

        let main_removal = manager
            .inspect_worktree_removal(&project.id, repository.to_str().unwrap())
            .await;
        assert!(matches!(main_removal, Err(AowError::Invalid(_))));
    }
}
