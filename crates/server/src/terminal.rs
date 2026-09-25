use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

use aow_protocol::{
    TerminalAgentList, TerminalAttachServerMessage, TerminalLayout, TerminalPane, TerminalPaneKind,
    TerminalPaneStatus, TerminalRuntime, TerminalRuntimeSpec, TerminalSplitAxis, TerminalTab,
    TerminalTabList,
};
use aow_terminald_client::{TerminaldAttachStream, TerminaldClient, TerminaldClientError};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{
        HeaderMap, StatusCode, Uri,
        header::{CONTENT_LENGTH, HOST, ORIGIN},
    },
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::{SecondsFormat, Utc};
use futures_util::{SinkExt, StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite;
use uuid::Uuid;

use crate::{AppState, HttpError, aow::AgentLaunch};

pub(crate) mod agent_control;
pub(crate) mod notifications;
mod rebuild;
mod session_titles;
mod sessions;

const METADATA_FILE: &str = "terminals.json";
const METADATA_VERSION: u32 = 1;
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const MAX_PANES_PER_TAB: usize = 64;
const MAX_LAYOUT_DEPTH: usize = 64;
const CLIPBOARD_DIRECTORY: &str = "clipboard-images";
const MAX_CLIPBOARD_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_CLIPBOARD_DIRECTORY_BYTES: u64 = 256 * 1024 * 1024;
const CLIPBOARD_IMAGE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const CLIPBOARD_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const BRIDGE_CLOSE_FORWARD_TIMEOUT: Duration = Duration::from_secs(1);
const BRIDGE_MESSAGE_FORWARD_TIMEOUT: Duration = Duration::from_secs(5);
const BRIDGE_RUNTIME_SYNC_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal tab not found: {0}")]
    TabNotFound(String),
    #[error("terminal pane not found: {0}")]
    PaneNotFound(String),
    #[error("invalid terminal request: {0}")]
    Invalid(String),
    #[error("terminal state conflict: {0}")]
    Conflict(String),
    #[error("WebSocket origin is not allowed: {0}")]
    ForbiddenOrigin(String),
    #[error("clipboard image exceeds the {MAX_CLIPBOARD_IMAGE_BYTES}-byte limit")]
    ClipboardImageTooLarge,
    #[error("clipboard image must be PNG, JPEG, GIF, or WebP")]
    UnsupportedClipboardImage,
    #[error("clipboard image storage quota is exhausted")]
    ClipboardQuotaExceeded,
    #[error("clipboard image storage is unavailable: {0}")]
    ClipboardStorage(String),
    #[error("terminald is unavailable: {0}")]
    DaemonUnavailable(String),
    #[error("terminald request failed: {0}")]
    Daemon(String),
    #[error("failed to start terminal: {0}")]
    RuntimeCreate(String),
    #[error("terminal state lock is poisoned")]
    Poisoned,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub(crate) struct TerminalManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    metadata_path: Option<PathBuf>,
    state: Mutex<ManagerState>,
    terminald: TerminaldClient,
    daemon_instance_id: Mutex<Option<String>>,
    operation: tokio::sync::Mutex<()>,
    runtime_sync: tokio::sync::Mutex<()>,
    agent_operations: Mutex<HashMap<String, std::sync::Weak<tokio::sync::RwLock<()>>>>,
    clipboard: ClipboardStorage,
    clipboard_operation: tokio::sync::Mutex<()>,
    task_stops: tokio::sync::broadcast::Sender<notifications::TaskStopNotification>,
    notifications_started: std::sync::atomic::AtomicBool,
}

struct ClipboardStorage {
    directory: PathBuf,
    remove_on_drop: bool,
    initialization_error: Option<String>,
}

#[derive(Default)]
struct ManagerState {
    tabs: Vec<TerminalTab>,
    removing_workspaces: HashSet<String>,
}

impl ManagerState {
    fn ensure_workspace_available(&self, path: &str) -> Result<(), TerminalError> {
        if self
            .removing_workspaces
            .iter()
            .any(|root| Path::new(path).starts_with(root))
        {
            return Err(TerminalError::Invalid(
                "Worktree 正在清理，暂时不能创建资源".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    version: u32,
    tabs: Vec<TerminalTab>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    workspace_root: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttachQuery {
    epoch: Option<String>,
    after: Option<u64>,
    control: Option<String>,
    capabilities: Option<String>,
    observer: Option<String>,
}

#[derive(Clone, Copy)]
struct AttachOptions<'a> {
    epoch: Option<&'a str>,
    after: Option<u64>,
    controlled: bool,
    vt_snapshot: bool,
    observer: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTerminalRequest {
    name: Option<String>,
    cwd: Option<String>,
    workspace_root: Option<String>,
    shell: Option<String>,
    agent_id: Option<String>,
    resume_session_id: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct UpdateTerminalRequest {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReorderTerminalsRequest {
    workspace_root: String,
    tab_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SplitTerminalRequest {
    target_pane_id: String,
    axis: TerminalSplitAxis,
    ratio: Option<f32>,
    cwd: Option<String>,
    shell: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct UpdateLayoutRequest {
    layout: TerminalLayout,
    revision: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ClipboardImageResponse {
    path: String,
    mime: &'static str,
    size: u64,
    expires_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClipboardImageType {
    mime: &'static str,
    extension: &'static str,
}

impl ClipboardStorage {
    fn persistent(state_dir: &Path) -> Result<Self, TerminalError> {
        let state_dir = absolute_path(state_dir)?;
        let directory = state_dir.join(CLIPBOARD_DIRECTORY);
        create_private_directory(&directory, true)?;
        let storage = Self {
            directory: std::fs::canonicalize(directory)?,
            remove_on_drop: false,
            initialization_error: None,
        };
        storage.clean_expired()?;
        Ok(storage)
    }

    fn temporary() -> Self {
        let parent = absolute_path(&std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
        for _ in 0..3 {
            let directory = parent.join(format!(
                "aow-clipboard-{}-{}",
                std::process::id(),
                Uuid::new_v4().as_simple()
            ));
            match create_private_directory(&directory, false) {
                Ok(()) => {
                    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
                    let storage = Self {
                        directory,
                        remove_on_drop: true,
                        initialization_error: None,
                    };
                    return match storage.clean_expired() {
                        Ok(_) => storage,
                        Err(error) => {
                            let mut storage = storage;
                            storage.initialization_error = Some(error.to_string());
                            storage
                        }
                    };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Self {
                        directory,
                        remove_on_drop: false,
                        initialization_error: Some(error.to_string()),
                    };
                }
            }
        }
        Self {
            directory: parent,
            remove_on_drop: false,
            initialization_error: Some(
                "could not allocate a unique temporary directory".to_owned(),
            ),
        }
    }

    fn clean_expired(&self) -> Result<u64, TerminalError> {
        self.clean_expired_at(SystemTime::now())
    }

    fn clean_expired_at(&self, now: SystemTime) -> Result<u64, TerminalError> {
        if let Some(error) = &self.initialization_error {
            return Err(TerminalError::ClipboardStorage(error.clone()));
        }
        let mut used_bytes = 0_u64;
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let expired = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= CLIPBOARD_IMAGE_TTL);
            if expired {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            } else {
                used_bytes = used_bytes.saturating_add(metadata.len());
            }
        }
        Ok(used_bytes)
    }
}

impl Drop for ClipboardStorage {
    fn drop(&mut self) {
        if self.remove_on_drop
            && let Err(error) = std::fs::remove_dir_all(&self.directory)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(
                %error,
                directory = %self.directory.display(),
                "failed to remove temporary clipboard image directory"
            );
        }
    }
}

impl TerminalManager {
    pub(crate) fn in_memory(terminald: TerminaldClient) -> Self {
        let clipboard = ClipboardStorage::temporary();
        let manager = Self {
            inner: Arc::new(ManagerInner {
                metadata_path: None,
                state: Mutex::new(ManagerState::default()),
                terminald,
                daemon_instance_id: Mutex::new(None),
                operation: tokio::sync::Mutex::new(()),
                runtime_sync: tokio::sync::Mutex::new(()),
                agent_operations: Mutex::new(HashMap::new()),
                clipboard,
                clipboard_operation: tokio::sync::Mutex::new(()),
                task_stops: tokio::sync::broadcast::channel(128).0,
                notifications_started: std::sync::atomic::AtomicBool::new(false),
            }),
        };
        manager.start_clipboard_cleanup();
        manager
    }

    pub(crate) fn persistent(
        state_dir: PathBuf,
        terminald: TerminaldClient,
    ) -> Result<Self, TerminalError> {
        std::fs::create_dir_all(&state_dir)?;
        let clipboard = ClipboardStorage::persistent(&state_dir)?;
        let metadata_path = state_dir.join(METADATA_FILE);
        let mut tabs = match std::fs::read(&metadata_path) {
            Ok(bytes) => {
                let persisted: PersistedState = serde_json::from_slice(&bytes)?;
                if persisted.version != METADATA_VERSION {
                    return Err(TerminalError::Invalid(format!(
                        "unsupported terminal metadata version {}",
                        persisted.version
                    )));
                }
                persisted.tabs
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        for tab in &mut tabs {
            for pane in &mut tab.panes {
                // Pane names are generated. Legacy custom labels are discarded.
                pane.name = default_pane_name(&pane.cwd, &pane.shell);
                if let Some(agent) = &mut pane.agent_terminal {
                    if agent.phase == aow_protocol::AgentTerminalPhase::Starting {
                        agent.phase = aow_protocol::AgentTerminalPhase::Failed;
                        agent.error = Some("AoW restarted during agent initialization".into());
                    }
                }
            }
            migrate_terminal_names(tab);
        }
        validate_persisted_tabs(&tabs)?;
        let manager = Self {
            inner: Arc::new(ManagerInner {
                metadata_path: Some(metadata_path),
                state: Mutex::new(ManagerState {
                    tabs,
                    ..ManagerState::default()
                }),
                terminald,
                daemon_instance_id: Mutex::new(None),
                operation: tokio::sync::Mutex::new(()),
                runtime_sync: tokio::sync::Mutex::new(()),
                agent_operations: Mutex::new(HashMap::new()),
                clipboard,
                clipboard_operation: tokio::sync::Mutex::new(()),
                task_stops: tokio::sync::broadcast::channel(128).0,
                notifications_started: std::sync::atomic::AtomicBool::new(false),
            }),
        };
        manager.start_clipboard_cleanup();
        Ok(manager)
    }

    fn start_clipboard_cleanup(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let inner = Arc::downgrade(&self.inner);
        handle.spawn(async move {
            let mut interval = tokio::time::interval(CLIPBOARD_CLEANUP_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(inner) = inner.upgrade() else {
                    break;
                };
                let _operation = inner.clipboard_operation.lock().await;
                if let Err(error) = inner.clipboard.clean_expired() {
                    tracing::warn!(%error, "failed to clean expired clipboard images");
                }
            }
        });
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, ManagerState>, TerminalError> {
        self.inner.state.lock().map_err(|_| TerminalError::Poisoned)
    }

    fn persist_locked(&self, state: &ManagerState) -> Result<(), TerminalError> {
        let Some(path) = &self.inner.metadata_path else {
            return Ok(());
        };
        atomic_save(
            path,
            &PersistedState {
                version: METADATA_VERSION,
                tabs: state.tabs.clone(),
            },
        )
    }

    pub(crate) async fn reconcile(&self) -> Result<(), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let before = self
                .inner
                .terminald
                .health()
                .await
                .map_err(map_client_error)?;
            let runtimes = self
                .inner
                .terminald
                .list()
                .await
                .map_err(map_client_error)?;
            let desired = {
                let state = self.lock_state()?;
                state
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.panes.iter().cloned())
                    .map(|pane| (pane.id.clone(), pane))
                    .collect::<HashMap<_, _>>()
            };
            let actual = runtimes
                .into_iter()
                .map(|runtime| (runtime.id.clone(), runtime))
                .collect::<HashMap<_, _>>();

            // The JSON document is the desired runtime set. Delete daemon-only
            // instances first so a prior control-plane crash cannot leak shells.
            for runtime_id in actual.keys().filter(|id| !desired.contains_key(*id)) {
                self.inner
                    .terminald
                    .delete(runtime_id)
                    .await
                    .map_err(map_client_error)?;
            }

            // Existing runtimes are deliberately preserved even if their live
            // size differs. Web restarts must not replace a healthy shell.
            // Every persisted pane is desired state (including one last seen
            // exited/interrupted), so daemon restart recreates all missing
            // panes as fresh running shells.
            for pane in desired.values() {
                if !actual.contains_key(&pane.id) && pane.restart_on_daemon_restart {
                    self.inner
                        .terminald
                        .create(&pane.id, &self.runtime_spec(pane))
                        .await
                        .map_err(map_client_error)?;
                }
            }
            self.mark_missing_nonrestart_panes(&desired, &actual)?;

            let mut reconciled = HashMap::new();
            for id in desired.keys() {
                if let Some(runtime) = self
                    .inner
                    .terminald
                    .get(id)
                    .await
                    .map_err(map_client_error)?
                {
                    reconciled.insert(id.clone(), runtime);
                }
            }
            let after = self
                .inner
                .terminald
                .health()
                .await
                .map_err(map_client_error)?;
            if before.instance_id != after.instance_id {
                tracing::warn!(
                    attempt,
                    before_instance_id = %before.instance_id,
                    after_instance_id = %after.instance_id,
                    "terminald restarted during reconciliation; retrying"
                );
                continue;
            }

            self.record_daemon_instance(after.instance_id)?;
            return self.apply_runtime_statuses(&reconciled);
        }
        Err(TerminalError::DaemonUnavailable(
            "terminald restarted repeatedly during reconciliation".to_owned(),
        ))
    }

    fn record_daemon_instance(&self, instance_id: String) -> Result<(), TerminalError> {
        let mut instance = self
            .inner
            .daemon_instance_id
            .lock()
            .map_err(|_| TerminalError::Poisoned)?;
        let previous = instance.replace(instance_id.clone());
        if previous.as_deref().is_some_and(|id| id != instance_id) {
            tracing::info!(
                previous_instance_id = ?previous,
                %instance_id,
                "terminald instance changed; desired runtimes were rebuilt"
            );
        }
        Ok(())
    }

    fn apply_runtime_statuses(
        &self,
        runtimes: &HashMap<String, TerminalRuntime>,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        let mut changed = false;
        for tab in &mut state.tabs {
            let mut tab_changed = false;
            for pane in &mut tab.panes {
                let Some(runtime) = runtimes.get(&pane.id) else {
                    continue;
                };
                if apply_runtime(pane, runtime) {
                    tab_changed = true;
                    changed = true;
                }
            }
            if tab_changed {
                // Runtime state and size are not structural layout changes.
                tab.updated_at = timestamp();
            }
        }
        if changed && let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(())
    }

    fn mark_missing_nonrestart_panes(
        &self,
        desired: &HashMap<String, TerminalPane>,
        actual: &HashMap<String, TerminalRuntime>,
    ) -> Result<(), TerminalError> {
        let missing = desired
            .values()
            .filter(|pane| !pane.restart_on_daemon_restart && !actual.contains_key(&pane.id))
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        let now = timestamp();
        let mut changed = false;
        for tab in &mut state.tabs {
            let mut tab_changed = false;
            for pane in &mut tab.panes {
                if missing.contains(pane.id.as_str()) && pane.status == TerminalPaneStatus::Running
                {
                    pane.status = TerminalPaneStatus::Interrupted;
                    pane.exit_code = None;
                    pane.updated_at = now.clone();
                    tab_changed = true;
                    changed = true;
                }
            }
            if tab_changed {
                tab.updated_at = now.clone();
            }
        }
        if changed && let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(())
    }

    async fn sync_runtime(&self, pane_id: &str) -> Result<(), TerminalError> {
        // Multiple browser bridges for one pane can overlap briefly during a
        // takeover. Serialize get+apply so every later ACK observes and stores
        // terminald's latest authoritative state rather than a stale snapshot.
        let _sync = self.inner.runtime_sync.lock().await;
        let runtime = self
            .inner
            .terminald
            .get(pane_id)
            .await
            .map_err(map_client_error)?;
        if let Some(runtime) = runtime {
            self.apply_runtime_statuses(&HashMap::from([(pane_id.to_owned(), runtime)]))?;
        }
        Ok(())
    }

    fn list_snapshot(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<TerminalTabList, TerminalError> {
        let state = self.lock_state()?;
        let tabs = state
            .tabs
            .iter()
            .filter(|tab| {
                workspace_root
                    .map(|root| tab.workspace_root == root)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        Ok(TerminalTabList { tabs })
    }

    fn get_snapshot(&self, tab_id: &str) -> Result<TerminalTab, TerminalError> {
        self.lock_state()?
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))
    }

    async fn list(&self, workspace_root: Option<&str>) -> Result<TerminalTabList, TerminalError> {
        self.reconcile().await?;
        self.list_snapshot(workspace_root)
    }

    async fn agents(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<TerminalAgentList, TerminalError> {
        let mut detected = self
            .inner
            .terminald
            .agents()
            .await
            .map_err(map_client_error)?;
        let tabs = self.list_snapshot(workspace_root)?.tabs;
        let panes: HashSet<_> = tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .map(|pane| &pane.id)
            .collect();
        detected.agents.retain(|id, _| panes.contains(id));
        detected.titles.retain(|id, _| panes.contains(id));
        detected.processes.retain(|id, _| panes.contains(id));
        session_titles::enrich(&mut detected).await;
        Ok(detected)
    }

    async fn get(&self, tab_id: &str) -> Result<TerminalTab, TerminalError> {
        self.reconcile().await?;
        self.get_snapshot(tab_id)
    }

    async fn create(&self, request: CreateTerminalRequest) -> Result<TerminalTab, TerminalError> {
        self.ensure_creation_available(&request)?;
        if request.resume_session_id.is_some() {
            return Err(TerminalError::Invalid(
                "resume_session_id requires agent_id".to_owned(),
            ));
        }
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        let requested_name = request.name.as_deref().map(validate_name).transpose()?;
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())?;
        validate_directory(&cwd).await?;
        let workspace_root = request
            .workspace_root
            .filter(|root| !root.trim().is_empty())
            .unwrap_or_else(|| cwd.clone());
        validate_directory(&workspace_root).await?;
        let shell = normalize_shell(request.shell.as_deref())?;
        if request.agent_id.is_some() {
            return Err(TerminalError::Invalid(
                "agent_id is only accepted by the AoW terminal endpoint".to_owned(),
            ));
        }
        let (rows, cols) = terminal_size(request.rows, request.cols)?;
        let pane_id = Uuid::new_v4().to_string();
        let tab_id = Uuid::new_v4().to_string();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            name: default_pane_name(&cwd, &shell),
            cwd,
            shell,
            arguments: Vec::new(),
            kind: TerminalPaneKind::Terminal,
            agent_id: None,
            agent_profile_id: None,
            agent_terminal: None,
            restart_on_daemon_restart: true,
            status: TerminalPaneStatus::Running,
            rows,
            cols,
            exit_code: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let tab = {
            let mut state = self.lock_state()?;
            state.ensure_workspace_available(&workspace_root)?;
            state.ensure_workspace_available(&pane.cwd)?;
            let name_is_custom = Some(requested_name.is_some());
            let name =
                requested_name.unwrap_or_else(|| format!("Terminal {}", state.tabs.len() + 1));
            let tab = TerminalTab {
                id: tab_id,
                name,
                name_is_custom,
                workspace_root,
                layout: TerminalLayout::Pane {
                    pane_id: pane_id.clone(),
                },
                panes: vec![pane.clone()],
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            state.tabs.push(tab.clone());
            if let Err(error) = self.persist_locked(&state) {
                state.tabs.pop();
                return Err(error);
            }
            tab
        };

        if let Err(error) = self
            .inner
            .terminald
            .create(&pane_id, &self.runtime_spec(&pane))
            .await
            .map_err(map_create_error)
        {
            if self
                .runtime_matches(&pane_id, &self.runtime_spec(&pane))
                .await?
            {
                return Ok(tab);
            }
            self.delete_failed_runtime_best_effort(&pane_id).await;
            self.rollback_created_tab(&tab.id)?;
            return Err(error);
        }
        Ok(tab)
    }

    async fn create_agent(
        &self,
        request: CreateTerminalRequest,
        launch: AgentLaunch,
    ) -> Result<TerminalTab, TerminalError> {
        self.create_agent_with_state(request, launch, None).await
    }

    async fn create_agent_with_state(
        &self,
        request: CreateTerminalRequest,
        mut launch: AgentLaunch,
        agent_terminal: Option<aow_protocol::AgentTerminalState>,
    ) -> Result<TerminalTab, TerminalError> {
        self.ensure_creation_available(&request)?;
        if let Some(session_id) = request.resume_session_id.as_deref() {
            launch
                .resume_session(session_id)
                .map_err(|error| TerminalError::Invalid(error.to_string()))?;
        }
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        if request.shell.is_some() {
            return Err(TerminalError::Invalid(
                "shell cannot be combined with agent_id".to_owned(),
            ));
        }
        let requested_name = request.name.as_deref().map(validate_name).transpose()?;
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())?;
        validate_directory(&cwd).await?;
        let workspace_root = request
            .workspace_root
            .filter(|root| !root.trim().is_empty())
            .unwrap_or_else(|| cwd.clone());
        validate_directory(&workspace_root).await?;
        let (rows, cols) = terminal_size(request.rows, request.cols)?;
        let pane_id = Uuid::new_v4().to_string();
        let tab_id = Uuid::new_v4().to_string();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            name: launch.display_name.clone(),
            cwd,
            shell: launch.executable,
            arguments: launch.args,
            kind: TerminalPaneKind::Agent,
            agent_id: Some(launch.agent_type.id().to_owned()),
            agent_profile_id: request.agent_id,
            agent_terminal,
            restart_on_daemon_restart: false,
            status: TerminalPaneStatus::Running,
            rows,
            cols,
            exit_code: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let tab = {
            let mut state = self.lock_state()?;
            state.ensure_workspace_available(&workspace_root)?;
            state.ensure_workspace_available(&pane.cwd)?;
            let tab = TerminalTab {
                id: tab_id,
                name_is_custom: Some(requested_name.is_some()),
                name: requested_name.unwrap_or_else(|| launch.display_name.clone()),
                workspace_root,
                layout: TerminalLayout::Pane {
                    pane_id: pane_id.clone(),
                },
                panes: vec![pane.clone()],
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            state.tabs.push(tab.clone());
            if let Err(error) = self.persist_locked(&state) {
                state.tabs.pop();
                return Err(error);
            }
            tab
        };
        let mut spec = self.runtime_spec(&pane);
        spec.environment = launch.env;
        if let Err(error) = self
            .inner
            .terminald
            .create(&pane_id, &spec)
            .await
            .map_err(map_create_error)
        {
            self.delete_failed_runtime_best_effort(&pane_id).await;
            self.rollback_created_tab(&tab.id)?;
            return Err(error);
        }
        Ok(tab)
    }

    async fn runtime_matches(
        &self,
        pane_id: &str,
        expected: &TerminalRuntimeSpec,
    ) -> Result<bool, TerminalError> {
        match self.inner.terminald.get(pane_id).await {
            Ok(Some(runtime)) => Ok(runtime.spec() == *expected),
            Ok(None) => Ok(false),
            Err(error) => {
                tracing::debug!(%error, %pane_id, "could not probe ambiguous terminal create result");
                Ok(false)
            }
        }
    }

    async fn delete_failed_runtime_best_effort(&self, pane_id: &str) {
        if let Err(error) = self.inner.terminald.delete(pane_id).await {
            // Once rollback removes this ID from desired JSON, a later
            // successful reconcile also recognizes it as an orphan.
            tracing::warn!(%error, %pane_id, "failed to clean up ambiguous terminal create");
        }
    }

    fn rollback_created_tab(&self, tab_id: &str) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let Some(index) = state.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Ok(());
        };
        let tab = state.tabs.remove(index);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs.insert(index, tab);
            return Err(error);
        }
        Ok(())
    }

    fn update(
        &self,
        tab_id: &str,
        request: UpdateTerminalRequest,
    ) -> Result<TerminalTab, TerminalError> {
        if request.name.is_none() {
            return Err(TerminalError::Invalid(
                "terminal update must include name".to_owned(),
            ));
        }
        let name = request.name.as_deref().map(validate_name).transpose()?;
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        if state.tabs[index]
            .panes
            .iter()
            .any(|pane| pane.agent_terminal.is_some())
        {
            return Err(TerminalError::Invalid(
                "CLI-created terminal tabs cannot be renamed".to_owned(),
            ));
        }
        let old = state.tabs[index].clone();
        if let Some(name) = name {
            state.tabs[index].name = name;
            state.tabs[index].name_is_custom = Some(true);
        }
        touch_tab(&mut state.tabs[index]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = old;
            return Err(error);
        }
        Ok(state.tabs[index].clone())
    }

    fn runtime_spec(&self, pane: &TerminalPane) -> TerminalRuntimeSpec {
        runtime_spec(pane)
    }

    fn reorder(&self, request: ReorderTerminalsRequest) -> Result<TerminalTabList, TerminalError> {
        if request.workspace_root.trim().is_empty() {
            return Err(TerminalError::Invalid(
                "workspace_root cannot be empty".to_owned(),
            ));
        }
        let requested_ids = request.tab_ids.iter().collect::<HashSet<_>>();
        if requested_ids.len() != request.tab_ids.len() {
            return Err(TerminalError::Invalid(
                "terminal order contains duplicate tab ids".to_owned(),
            ));
        }

        let mut state = self.lock_state()?;
        let workspace_indices = state
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                (tab.workspace_root == request.workspace_root).then_some(index)
            })
            .collect::<Vec<_>>();
        let current_ids = workspace_indices
            .iter()
            .map(|index| state.tabs[*index].id.as_str())
            .collect::<HashSet<_>>();
        if requested_ids.len() != current_ids.len()
            || !requested_ids
                .iter()
                .all(|tab_id| current_ids.contains(tab_id.as_str()))
        {
            return Err(TerminalError::Conflict(
                "terminal order is stale; refresh the terminal list and try again".to_owned(),
            ));
        }

        let requested_order = request
            .tab_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let current_order = workspace_indices
            .iter()
            .map(|index| state.tabs[*index].id.as_str())
            .collect::<Vec<_>>();
        if current_order == requested_order {
            return Ok(TerminalTabList {
                tabs: workspace_indices
                    .iter()
                    .map(|index| state.tabs[*index].clone())
                    .collect(),
            });
        }

        let reordered = state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_root == request.workspace_root)
            .map(|tab| (tab.id.clone(), tab.clone()))
            .collect::<HashMap<_, _>>();
        let old = state.tabs.clone();
        for (index, tab_id) in workspace_indices.iter().zip(&request.tab_ids) {
            state.tabs[*index] = reordered
                .get(tab_id)
                .expect("validated terminal order must contain every tab")
                .clone();
        }
        if let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(TerminalTabList {
            tabs: workspace_indices
                .iter()
                .map(|index| state.tabs[*index].clone())
                .collect(),
        })
    }

    async fn delete_tab(&self, tab_id: &str) -> Result<(), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let pane_ids = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            let tab = state.tabs.remove(index);
            if let Err(error) = self.persist_locked(&state) {
                state.tabs.insert(index, tab);
                return Err(error);
            }
            tab.panes
                .into_iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>()
        };
        for pane_id in pane_ids {
            self.inner
                .terminald
                .delete(&pane_id)
                .await
                .map_err(map_client_error)?;
        }
        Ok(())
    }

    pub(crate) fn workspace_tab_counts(
        &self,
        workspace_root: &str,
    ) -> Result<(usize, usize), TerminalError> {
        let state = self.lock_state()?;
        let matching = state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_root == workspace_root);
        let mut terminal_tabs = 0;
        let mut agent_tabs = 0;
        for tab in matching {
            if tab
                .panes
                .iter()
                .any(|pane| pane.kind == TerminalPaneKind::Agent)
            {
                agent_tabs += 1;
            } else {
                terminal_tabs += 1;
            }
        }
        Ok((terminal_tabs, agent_tabs))
    }

    fn ensure_creation_available(
        &self,
        request: &CreateTerminalRequest,
    ) -> Result<(), TerminalError> {
        let state = self.lock_state()?;
        if let Some(root) = &request.workspace_root {
            state.ensure_workspace_available(root)?;
        }
        if let Some(cwd) = &request.cwd {
            state.ensure_workspace_available(cwd)?;
        }
        Ok(())
    }

    pub(crate) fn set_workspace_removing(
        &self,
        workspace_root: &str,
        removing: bool,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        if removing {
            state.removing_workspaces.insert(workspace_root.to_owned());
        } else {
            state.removing_workspaces.remove(workspace_root);
        }
        Ok(())
    }

    pub(crate) async fn delete_workspace(
        &self,
        workspace_root: &str,
    ) -> Result<(usize, usize), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let (tab_ids, pane_ids, terminal_tabs, agent_tabs) = {
            let state = self.lock_state()?;
            let tabs = state
                .tabs
                .iter()
                .filter(|tab| tab.workspace_root == workspace_root)
                .cloned()
                .collect::<Vec<_>>();
            let agent_tabs = tabs
                .iter()
                .filter(|tab| {
                    tab.panes
                        .iter()
                        .any(|pane| pane.kind == TerminalPaneKind::Agent)
                })
                .count();
            let tab_ids = tabs
                .iter()
                .map(|tab| tab.id.clone())
                .collect::<HashSet<_>>();
            let pane_ids = tabs
                .iter()
                .flat_map(|tab| tab.panes.iter().map(|pane| pane.id.clone()))
                .collect::<Vec<_>>();
            let terminal_tabs = tabs.len() - agent_tabs;
            (tab_ids, pane_ids, terminal_tabs, agent_tabs)
        };
        if tab_ids.is_empty() {
            return Ok((0, 0));
        }

        // Stop and reap every runtime before removing durable desired state.
        // If a daemon call fails, the worktree removal is aborted and durable
        // metadata remains available for an explicit retry.
        for pane_id in pane_ids {
            self.inner
                .terminald
                .delete(&pane_id)
                .await
                .map_err(map_client_error)?;
        }

        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        state.tabs.retain(|tab| !tab_ids.contains(&tab.id));
        if let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok((terminal_tabs, agent_tabs))
    }

    async fn split(
        &self,
        tab_id: &str,
        request: SplitTerminalRequest,
    ) -> Result<TerminalTab, TerminalError> {
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        let (target, target_revision) = {
            let state = self.lock_state()?;
            let tab = state
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
            if tab.panes.len() >= MAX_PANES_PER_TAB {
                return Err(TerminalError::Invalid(format!(
                    "a tab may contain at most {MAX_PANES_PER_TAB} panes"
                )));
            }
            let target = tab
                .panes
                .iter()
                .find(|pane| pane.id == request.target_pane_id)
                .cloned()
                .ok_or_else(|| TerminalError::PaneNotFound(request.target_pane_id.clone()))?;
            (target, tab.revision)
        };
        let ratio = validate_ratio(request.ratio.unwrap_or(0.5))?;
        let cwd = request.cwd.unwrap_or_else(|| target.cwd.clone());
        validate_directory(&cwd).await?;
        let inherited_shell =
            (target.kind == TerminalPaneKind::Terminal).then_some(target.shell.as_str());
        let shell = normalize_shell(request.shell.as_deref().or(inherited_shell))?;
        let (rows, cols) = terminal_size(
            request.rows.or(Some(target.rows)),
            request.cols.or(Some(target.cols)),
        )?;
        let pane_id = Uuid::new_v4().to_string();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            name: default_pane_name(&cwd, &shell),
            cwd,
            shell,
            arguments: Vec::new(),
            kind: TerminalPaneKind::Terminal,
            agent_id: None,
            agent_profile_id: None,
            agent_terminal: None,
            restart_on_daemon_restart: true,
            status: TerminalPaneStatus::Running,
            rows,
            cols,
            exit_code: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let (tab, previous_tab) = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            state.ensure_workspace_available(&state.tabs[index].workspace_root)?;
            state.ensure_workspace_available(&pane.cwd)?;
            if state.tabs[index].revision != target_revision {
                return Err(TerminalError::Conflict(
                    "terminal layout changed while the new pane was being prepared".to_owned(),
                ));
            }
            let old = state.tabs[index].clone();
            let replacement = TerminalLayout::Split {
                axis: request.axis,
                ratio,
                first: Box::new(TerminalLayout::Pane {
                    pane_id: request.target_pane_id.clone(),
                }),
                second: Box::new(TerminalLayout::Pane {
                    pane_id: pane_id.clone(),
                }),
            };
            if !replace_layout_leaf(
                &mut state.tabs[index].layout,
                &request.target_pane_id,
                &replacement,
            ) {
                return Err(TerminalError::Conflict(
                    "the target pane changed while the new pane was being prepared".to_owned(),
                ));
            }
            state.tabs[index].panes.push(pane.clone());
            touch_tab(&mut state.tabs[index]);
            if let Err(error) = self.persist_locked(&state) {
                state.tabs[index] = old;
                return Err(error);
            }
            (state.tabs[index].clone(), old)
        };

        if let Err(error) = self
            .inner
            .terminald
            .create(&pane_id, &self.runtime_spec(&pane))
            .await
            .map_err(map_create_error)
        {
            if self
                .runtime_matches(&pane_id, &self.runtime_spec(&pane))
                .await?
            {
                return Ok(tab);
            }
            self.delete_failed_runtime_best_effort(&pane_id).await;
            self.rollback_split(tab_id, &pane_id, &tab, previous_tab)?;
            return Err(error);
        }
        Ok(tab)
    }

    fn rollback_split(
        &self,
        tab_id: &str,
        pane_id: &str,
        created_tab: &TerminalTab,
        previous_tab: TerminalTab,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        let failed_tab = state.tabs[index].clone();
        if failed_tab.revision == created_tab.revision {
            let mut restored = previous_tab;
            // Preserve runtime metadata that can change without incrementing
            // the structural revision while terminald create is in flight.
            for pane in &mut restored.panes {
                if let Some(current) = failed_tab.panes.iter().find(|item| item.id == pane.id) {
                    pane.status = current.status;
                    pane.rows = current.rows;
                    pane.cols = current.cols;
                    pane.exit_code = current.exit_code;
                    pane.updated_at.clone_from(&current.updated_at);
                }
            }
            if failed_tab.updated_at != created_tab.updated_at {
                restored.updated_at.clone_from(&failed_tab.updated_at);
            }
            state.tabs[index] = restored;
        } else {
            // A concurrent rename/layout update committed. Remove only the
            // failed pane from the latest tab instead of overwriting it with
            // the stale pre-split snapshot.
            state.tabs[index].layout =
                remove_layout_leaf(state.tabs[index].layout.clone(), pane_id).ok_or_else(|| {
                    TerminalError::Conflict(
                        "failed terminal pane disappeared during split rollback".to_owned(),
                    )
                })?;
            state.tabs[index].panes.retain(|pane| pane.id != pane_id);
            touch_tab(&mut state.tabs[index]);
        }
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = failed_tab;
            return Err(error);
        }
        Ok(())
    }

    async fn delete_pane(
        &self,
        tab_id: &str,
        pane_id: &str,
    ) -> Result<Option<TerminalTab>, TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let result = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            if !state.tabs[index]
                .panes
                .iter()
                .any(|pane| pane.id == pane_id)
            {
                return Err(TerminalError::PaneNotFound(pane_id.to_owned()));
            }
            let old = state.tabs[index].clone();
            let result = if old.panes.len() == 1 {
                state.tabs.remove(index);
                None
            } else {
                state.tabs[index].layout = remove_layout_leaf(old.layout.clone(), pane_id)
                    .ok_or_else(|| TerminalError::Invalid("layout cannot be empty".to_owned()))?;
                state.tabs[index].panes.retain(|pane| pane.id != pane_id);
                touch_tab(&mut state.tabs[index]);
                Some(state.tabs[index].clone())
            };
            if let Err(error) = self.persist_locked(&state) {
                if result.is_none() {
                    state.tabs.insert(index, old);
                } else {
                    state.tabs[index] = old;
                }
                return Err(error);
            }
            result
        };
        self.inner
            .terminald
            .delete(pane_id)
            .await
            .map_err(map_client_error)?;
        Ok(result)
    }

    fn update_layout(
        &self,
        tab_id: &str,
        request: UpdateLayoutRequest,
    ) -> Result<TerminalTab, TerminalError> {
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        if request
            .revision
            .is_some_and(|revision| revision != state.tabs[index].revision)
        {
            return Err(TerminalError::Conflict(format!(
                "layout revision is stale; current revision is {}",
                state.tabs[index].revision
            )));
        }
        validate_layout(&request.layout, &state.tabs[index].panes)?;
        let old = state.tabs[index].clone();
        state.tabs[index].layout = request.layout;
        touch_tab(&mut state.tabs[index]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = old;
            return Err(error);
        }
        Ok(state.tabs[index].clone())
    }

    fn ensure_pane(&self, tab_id: &str, pane_id: &str) -> Result<(), TerminalError> {
        let state = self.lock_state()?;
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
        if tab.panes.iter().any(|pane| pane.id == pane_id) {
            Ok(())
        } else {
            Err(TerminalError::PaneNotFound(pane_id.to_owned()))
        }
    }

    async fn store_clipboard_image(
        &self,
        tab_id: &str,
        pane_id: &str,
        body: Body,
    ) -> Result<ClipboardImageResponse, TerminalError> {
        self.ensure_pane(tab_id, pane_id)?;
        let _operation = self.inner.clipboard_operation.lock().await;
        let used_bytes = self.inner.clipboard.clean_expired()?;
        let upload_id = Uuid::new_v4();
        let temp_path = self
            .inner
            .clipboard
            .directory
            .join(format!(".{}.tmp", upload_id.as_simple()));
        let result = self
            .write_clipboard_image(tab_id, pane_id, body, upload_id, &temp_path, used_bytes)
            .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        result
    }

    async fn write_clipboard_image(
        &self,
        tab_id: &str,
        pane_id: &str,
        body: Body,
        upload_id: Uuid,
        temp_path: &Path,
        used_bytes: u64,
    ) -> Result<ClipboardImageResponse, TerminalError> {
        let mut file = open_private_file(temp_path).await?;
        let mut stream = body.into_data_stream();
        let mut size = 0_u64;
        let mut image_bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                TerminalError::Invalid(format!("invalid request body: {error}"))
            })?;
            size = size
                .checked_add(chunk.len() as u64)
                .ok_or(TerminalError::ClipboardImageTooLarge)?;
            if size > MAX_CLIPBOARD_IMAGE_BYTES {
                return Err(TerminalError::ClipboardImageTooLarge);
            }
            if used_bytes.saturating_add(size) > MAX_CLIPBOARD_DIRECTORY_BYTES {
                return Err(TerminalError::ClipboardQuotaExceeded);
            }
            image_bytes.extend_from_slice(&chunk);
            file.write_all(&chunk).await?;
        }
        let image_type =
            clipboard_image_type(&image_bytes).ok_or(TerminalError::UnsupportedClipboardImage)?;
        file.sync_all().await?;
        drop(file);
        self.ensure_pane(tab_id, pane_id)?;

        let final_path = self.inner.clipboard.directory.join(format!(
            "{}.{}",
            upload_id.as_simple(),
            image_type.extension
        ));
        {
            let state = self.lock_state()?;
            let tab = state
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
            if !tab.panes.iter().any(|pane| pane.id == pane_id) {
                return Err(TerminalError::PaneNotFound(pane_id.to_owned()));
            }
            std::fs::rename(temp_path, &final_path)?;
        }
        let expires_at = (Utc::now() + chrono::Duration::from_std(CLIPBOARD_IMAGE_TTL).unwrap())
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        Ok(ClipboardImageResponse {
            path: final_path.to_string_lossy().into_owned(),
            mime: image_type.mime,
            size,
            expires_at,
        })
    }

    async fn attach(
        &self,
        tab_id: &str,
        pane_id: &str,
        options: AttachOptions<'_>,
    ) -> Result<TerminaldAttachStream, TerminalError> {
        self.ensure_pane(tab_id, pane_id)?;
        let controlled = options.controlled || self.cli_agent(pane_id).is_ok();
        let attachment = if controlled {
            self.inner
                .terminald
                .attach_controlled_with_resume_capabilities_and_observer(
                    pane_id,
                    options.epoch,
                    options.after,
                    options.vt_snapshot,
                    options.observer,
                )
                .await
        } else {
            self.inner
                .terminald
                .attach_with_resume_capabilities(
                    pane_id,
                    options.epoch,
                    options.after,
                    options.vt_snapshot,
                )
                .await
        };
        match attachment {
            Ok(socket) => return Ok(socket),
            Err(error) if client_error_is_not_found(&error) => {
                // The daemon can restart between page load and WS upgrade.
                self.reconcile().await?;
            }
            Err(error) => return Err(map_client_error(error)),
        }
        // Reconcile may have recreated this pane as a new runtime. Never reuse
        // a cursor from the previous terminald instance, but preserve the
        // browser's requested control protocol across the retry.
        let attachment = if controlled {
            self.inner
                .terminald
                .attach_controlled_with_resume_capabilities_and_observer(
                    pane_id,
                    None,
                    None,
                    options.vt_snapshot,
                    options.observer,
                )
                .await
        } else {
            self.inner
                .terminald
                .attach_with_resume_capabilities(pane_id, None, None, options.vt_snapshot)
                .await
        };
        attachment.map_err(|error| {
            if client_error_is_not_found(&error) {
                TerminalError::DaemonUnavailable(
                    "terminal runtime disappeared while attaching".to_owned(),
                )
            } else {
                map_client_error(error)
            }
        })
    }

    fn apply_attach_status(
        &self,
        pane_id: &str,
        status: TerminalPaneStatus,
        exit_code: Option<u32>,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let Some((tab_index, pane_index)) = find_pane(&state.tabs, pane_id) else {
            return Ok(());
        };
        let pane = &state.tabs[tab_index].panes[pane_index];
        if pane.status == status && pane.exit_code == exit_code {
            return Ok(());
        }
        let old = state.tabs[tab_index].clone();
        let now = timestamp();
        let pane = &mut state.tabs[tab_index].panes[pane_index];
        pane.status = status;
        pane.exit_code = exit_code;
        pane.updated_at.clone_from(&now);
        state.tabs[tab_index].updated_at = now;
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[tab_index] = old;
            return Err(error);
        }
        Ok(())
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/terminals", get(list_terminals).post(create_terminal))
        .route("/api/terminals/order", put(reorder_terminals))
        .route("/api/terminals/agents", get(list_terminal_agents))
        .route("/api/terminals/task-stops", get(notifications::events))
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/agent-sessions",
            get(sessions::list),
        )
        .route(
            "/api/terminals/{tab_id}",
            get(get_terminal)
                .patch(update_terminal)
                .delete(delete_terminal),
        )
        .route("/api/terminals/{tab_id}/split", post(split_terminal))
        .route(
            "/api/terminals/{tab_id}/rebuild",
            post(rebuild::rebuild_terminal),
        )
        .route(
            "/api/terminals/{tab_id}/layout",
            put(update_terminal_layout),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}",
            axum::routing::delete(delete_terminal_pane),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/ws",
            get(attach_terminal_pane),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/clipboard-images",
            post(upload_terminal_clipboard_image),
        )
}

async fn list_terminals(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TerminalTabList>, HttpError> {
    Ok(Json(
        state
            .terminals
            .list(query.workspace_root.as_deref())
            .await
            .map_err(terminal_http_error)?,
    ))
}

async fn list_terminal_agents(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TerminalAgentList>, HttpError> {
    state
        .terminals
        .agents(query.workspace_root.as_deref())
        .await
        .map(Json)
        .map_err(terminal_http_error)
}

async fn get_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .get(&tab_id)
            .await
            .map_err(terminal_http_error)?,
    ))
}

async fn create_terminal(
    State(state): State<AppState>,
    Json(request): Json<CreateTerminalRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let tab = if let Some(agent_id) = request.agent_id.clone() {
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())
            .map_err(terminal_http_error)?;
        // Native sessions can originate in a worktree subdirectory. Resolve the
        // profile against the registered workspace while retaining that session cwd.
        let launch_workspace = if request.resume_session_id.is_some() {
            request.workspace_root.as_deref().unwrap_or(&cwd)
        } else {
            &cwd
        };
        let launch = state
            .aow
            .resolve_agent_launch(&agent_id, launch_workspace)
            .await
            .map_err(crate::aow::aow_http_error)?;
        state.terminals.create_agent(request, launch).await
    } else {
        state.terminals.create(request).await
    }
    .map_err(terminal_http_error)?;
    Ok((StatusCode::CREATED, Json(tab)))
}

async fn update_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<UpdateTerminalRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .update(&tab_id, request)
            .map_err(terminal_http_error)?,
    ))
}

async fn reorder_terminals(
    State(state): State<AppState>,
    Json(request): Json<ReorderTerminalsRequest>,
) -> Result<Json<TerminalTabList>, HttpError> {
    Ok(Json(
        state
            .terminals
            .reorder(request)
            .map_err(terminal_http_error)?,
    ))
}

async fn delete_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<StatusCode, HttpError> {
    state
        .terminals
        .delete_tab(&tab_id)
        .await
        .map_err(terminal_http_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn split_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<SplitTerminalRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .split(&tab_id, request)
            .await
            .map_err(terminal_http_error)?,
    ))
}

async fn delete_terminal_pane(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
) -> Result<Response, HttpError> {
    match state
        .terminals
        .delete_pane(&tab_id, &pane_id)
        .await
        .map_err(terminal_http_error)?
    {
        Some(tab) => Ok(Json(tab).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

async fn update_terminal_layout(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<UpdateLayoutRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .update_layout(&tab_id, request)
            .map_err(terminal_http_error)?,
    ))
}

async fn attach_terminal_pane(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
    Query(query): Query<AttachQuery>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    validate_request_origin(&headers).map_err(terminal_http_error)?;
    state
        .terminals
        .ensure_pane(&tab_id, &pane_id)
        .map_err(terminal_http_error)?;
    let manager = state.terminals.clone();
    Ok(websocket
        .on_upgrade(move |socket| async move {
            match manager
                .attach(
                    &tab_id,
                    &pane_id,
                    AttachOptions {
                        epoch: query.epoch.as_deref(),
                        after: query.after,
                        controlled: query.control.as_deref() == Some("v2"),
                        vt_snapshot: query.capabilities.as_deref() == Some("vt-snapshot-v1"),
                        observer: query.observer.as_deref() == Some("v1"),
                    },
                )
                .await
            {
                Ok(daemon) => bridge_terminal_socket(socket, daemon, manager, pane_id).await,
                Err(error) => reject_terminal_socket(socket, error).await,
            }
        })
        .into_response())
}

async fn reject_terminal_socket(mut browser: WebSocket, error: TerminalError) {
    let text = serde_json::to_string(&TerminalAttachServerMessage::Error {
        code: "attach_failed".to_owned(),
        message: error.to_string(),
    })
    .expect("terminal attach error serialization cannot fail");
    let _ = browser.send(Message::Text(text.into())).await;
    let _ = browser.close().await;
}

async fn upload_terminal_clipboard_image(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, HttpError> {
    validate_request_origin(&headers).map_err(terminal_http_error)?;
    if let Some(content_length) = headers.get(CONTENT_LENGTH) {
        let content_length = content_length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                terminal_http_error(TerminalError::Invalid(
                    "Content-Length must be a non-negative integer".to_owned(),
                ))
            })?;
        if content_length > MAX_CLIPBOARD_IMAGE_BYTES {
            return Err(terminal_http_error(TerminalError::ClipboardImageTooLarge));
        }
    }
    let image = state
        .terminals
        .store_clipboard_image(&tab_id, &pane_id, body)
        .await
        .map_err(terminal_http_error)?;
    Ok((StatusCode::CREATED, Json(image)))
}

pub(crate) fn validate_request_origin(headers: &HeaderMap) -> Result<(), TerminalError> {
    let Some(origin) = headers.get(ORIGIN) else {
        return Ok(());
    };
    let origin = origin
        .to_str()
        .map_err(|_| TerminalError::ForbiddenOrigin("Origin is not valid ASCII".to_owned()))?;
    let origin: Uri = origin
        .parse()
        .map_err(|_| TerminalError::ForbiddenOrigin("Origin is not a valid URL".to_owned()))?;
    if !matches!(origin.scheme_str(), Some("http" | "https"))
        || origin.path() != "/"
        || origin.query().is_some()
    {
        return Err(TerminalError::ForbiddenOrigin(
            "Origin must be an HTTP origin without a path or query".to_owned(),
        ));
    }
    let origin_authority = origin.authority().ok_or_else(|| {
        TerminalError::ForbiddenOrigin("Origin does not contain a host".to_owned())
    })?;
    let host = headers
        .get(HOST)
        .ok_or_else(|| TerminalError::ForbiddenOrigin("Host header is required".to_owned()))?
        .to_str()
        .map_err(|_| TerminalError::ForbiddenOrigin("Host is not valid ASCII".to_owned()))?
        .parse::<http::uri::Authority>()
        .map_err(|_| TerminalError::ForbiddenOrigin("Host is invalid".to_owned()))?;
    if !origin_authority.host().eq_ignore_ascii_case(host.host())
        || origin_authority.port_u16() != host.port_u16()
    {
        return Err(TerminalError::ForbiddenOrigin(format!(
            "Origin authority {origin_authority} does not match Host {host}"
        )));
    }
    Ok(())
}

async fn bridge_terminal_socket(
    browser: WebSocket,
    daemon: TerminaldAttachStream,
    manager: TerminalManager,
    pane_id: String,
) {
    let (mut browser_tx, mut browser_rx) = browser.split();
    let (mut daemon_tx, mut daemon_rx) = daemon.split();
    let mut resize_syncs = FuturesUnordered::new();
    let mut resize_sync_pending = false;
    let cli_pane = manager.cli_agent(&pane_id).is_ok();
    let mut browser_control_guard = None;
    loop {
        tokio::select! {
            browser_message = browser_rx.next() => match browser_message {
                Some(Ok(message)) => {
                    let close = matches!(message, Message::Close(_));
                    let mut message = axum_to_tungstenite(message);
                    if cli_pane && !close {
                        use aow_protocol::TerminalAttachClientMessage as ClientMessage;
                        let command = if let tungstenite::Message::Text(text) = &message {
                            serde_json::from_str::<ClientMessage>(text).ok()
                        } else { None };
                        match command {
                            Some(ClientMessage::Claim { force: false }) if browser_control_guard.is_none() => {
                                message = tungstenite::Message::Text(serde_json::to_string(&ClientMessage::Observe).unwrap().into());
                            }
                            Some(ClientMessage::Claim { force: true }) => {
                                let lease = manager.agent_operation(&pane_id).and_then(|operation| operation.try_read_owned().map_err(|_| TerminalError::Conflict("Agent 正在初始化或提交任务，只能旁观".into())));
                                if manager.cli_agent(&pane_id).is_ok_and(|info| info.state.phase == aow_protocol::AgentTerminalPhase::Ready) && lease.is_ok() {
                                    browser_control_guard = lease.ok();
                                } else {
                                    // Reconnect as an observer; never let force bypass initialization.
                                    message = tungstenite::Message::Text(serde_json::to_string(&ClientMessage::Observe).unwrap().into());
                                }
                            }
                            Some(ClientMessage::Observe) => { browser_control_guard = None; }
                            Some(ClientMessage::Resize { .. } | ClientMessage::Write { .. }) | None
                                if browser_control_guard.is_none() && !matches!(message, tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) => {
                                    continue;
                                }
                            _ => {}
                        }
                    }
                    if close {
                        match send_bridge_message(
                            &mut daemon_tx,
                            message,
                            BRIDGE_CLOSE_FORWARD_TIMEOUT,
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, %pane_id, "failed to forward browser close to terminald");
                            }
                            Err(_) => {
                                tracing::debug!(%pane_id, "timed out forwarding browser close to terminald");
                            }
                        }
                        break;
                    }
                    match send_bridge_message(
                        &mut daemon_tx,
                        message,
                        BRIDGE_MESSAGE_FORWARD_TIMEOUT,
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::debug!(%error, %pane_id, "failed to forward browser message to terminald");
                            break;
                        }
                        Err(_) => {
                            tracing::debug!(%pane_id, "timed out forwarding browser message to terminald");
                            break;
                        }
                    }
                }
                Some(Err(_)) | None => break,
            },
            daemon_message = daemon_rx.next() => match daemon_message {
                Some(Ok(tungstenite::Message::Frame(_))) => {}
                Some(Ok(message)) => {
                    let mut sync_after_forward = false;
                    if let tungstenite::Message::Text(text) = &message
                        && let Ok(control) = serde_json::from_str::<TerminalAttachServerMessage>(text)
                    {
                        let result = match control {
                            TerminalAttachServerMessage::Status { status, exit_code } => {
                                manager.apply_attach_status(&pane_id, status, exit_code)
                            }
                            TerminalAttachServerMessage::Resized { .. } => {
                                // ACKs from superseded and current bridges can arrive in either
                                // order. Read terminald's authoritative size so a late old ACK
                                // can never overwrite the newest controller's dimensions. Defer
                                // starting the query until after the ACK is forwarded.
                                sync_after_forward = true;
                                Ok(())
                            }
                            TerminalAttachServerMessage::Written { .. }
                            | TerminalAttachServerMessage::Control { .. }
                            | TerminalAttachServerMessage::Stream { .. }
                            | TerminalAttachServerMessage::Error { .. } => Ok(()),
                        };
                        if let Err(error) = result {
                            tracing::warn!(%error, %pane_id, "failed to persist terminal control state");
                        }
                    }
                    let close = matches!(message, tungstenite::Message::Close(_));
                    let message = tungstenite_to_axum(message);
                    if close {
                        match send_bridge_message(
                            &mut browser_tx,
                            message,
                            BRIDGE_CLOSE_FORWARD_TIMEOUT,
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, %pane_id, "failed to forward terminald close to browser");
                            }
                            Err(_) => {
                                tracing::debug!(%pane_id, "timed out forwarding terminald close to browser");
                            }
                        }
                        break;
                    }
                    match send_bridge_message(
                        &mut browser_tx,
                        message,
                        BRIDGE_MESSAGE_FORWARD_TIMEOUT,
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::debug!(%error, %pane_id, "failed to forward terminald message to browser");
                            break;
                        }
                        Err(_) => {
                            tracing::debug!(%pane_id, "timed out forwarding terminald message to browser");
                            break;
                        }
                    }
                    if sync_after_forward {
                        // Keep at most one query in flight and one coalesced follow-up so resize
                        // persistence never stalls socket forwarding or grows without bound.
                        if resize_syncs.is_empty() {
                            resize_syncs.push(sync_bridge_runtime(
                                manager.clone(),
                                pane_id.clone(),
                                "after resize",
                            ));
                        } else {
                            resize_sync_pending = true;
                        }
                    }
                }
                Some(Err(error)) => {
                    tracing::debug!(%error, %pane_id, "terminald attach stream stopped");
                    break;
                }
                None => break,
            },
            Some(()) = resize_syncs.next() => {
                if resize_sync_pending {
                    resize_sync_pending = false;
                    resize_syncs.push(sync_bridge_runtime(
                        manager.clone(),
                        pane_id.clone(),
                        "after coalesced resize",
                    ));
                }
            },
        }
    }
    // Release the daemon attachment before doing any follow-up API request. In
    // particular, a disconnected browser must relinquish terminal control even
    // if runtime synchronization is delayed or terminald is unavailable.
    drop(daemon_tx);
    drop(daemon_rx);
    // Cancel any in-flight resize query before the final authoritative query.
    // The future is owned by this bridge, so it cannot outlive the attachment.
    drop(resize_syncs);
    sync_bridge_runtime(manager, pane_id, "after attach").await;
}

async fn send_bridge_message<S, Item>(
    sink: &mut S,
    message: Item,
    timeout: Duration,
) -> Result<Result<(), S::Error>, tokio::time::error::Elapsed>
where
    S: futures_util::Sink<Item> + Unpin,
{
    tokio::time::timeout(timeout, sink.send(message)).await
}

async fn sync_bridge_runtime(manager: TerminalManager, pane_id: String, context: &'static str) {
    match bounded_sync_runtime(&manager, &pane_id, BRIDGE_RUNTIME_SYNC_TIMEOUT).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, %pane_id, context, "failed to persist terminal runtime status");
        }
        Err(_) => {
            tracing::warn!(%pane_id, context, "timed out persisting terminal runtime status");
        }
    }
}

async fn bounded_sync_runtime(
    manager: &TerminalManager,
    pane_id: &str,
    timeout: Duration,
) -> Result<Result<(), TerminalError>, tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, manager.sync_runtime(pane_id)).await
}

fn axum_to_tungstenite(message: Message) -> tungstenite::Message {
    match message {
        Message::Text(text) => tungstenite::Message::Text(text.to_string().into()),
        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
        Message::Ping(bytes) => tungstenite::Message::Ping(bytes),
        Message::Pong(bytes) => tungstenite::Message::Pong(bytes),
        Message::Close(frame) => {
            tungstenite::Message::Close(frame.map(|frame| tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }))
        }
    }
}

fn tungstenite_to_axum(message: tungstenite::Message) -> Message {
    match message {
        tungstenite::Message::Text(text) => Message::Text(text.to_string().into()),
        tungstenite::Message::Binary(bytes) => Message::Binary(bytes),
        tungstenite::Message::Ping(bytes) => Message::Ping(bytes),
        tungstenite::Message::Pong(bytes) => Message::Pong(bytes),
        tungstenite::Message::Close(frame) => {
            Message::Close(frame.map(|frame| axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }))
        }
        tungstenite::Message::Frame(_) => unreachable!("raw frames are filtered by the bridge"),
    }
}

fn map_client_error(error: TerminaldClientError) -> TerminalError {
    match &error {
        TerminaldClientError::Io(_)
        | TerminaldClientError::Hyper(_)
        | TerminaldClientError::WebSocket(_) => TerminalError::DaemonUnavailable(error.to_string()),
        TerminaldClientError::HttpStatus { status, .. } if *status == StatusCode::CONFLICT => {
            TerminalError::Conflict(error.to_string())
        }
        _ => TerminalError::Daemon(error.to_string()),
    }
}

fn map_create_error(error: TerminaldClientError) -> TerminalError {
    match map_client_error(error) {
        TerminalError::Daemon(message) => TerminalError::RuntimeCreate(message),
        error => error,
    }
}

fn client_error_is_not_found(error: &TerminaldClientError) -> bool {
    matches!(
        error,
        TerminaldClientError::HttpStatus { status, .. } if *status == StatusCode::NOT_FOUND
    ) || matches!(
        error,
        TerminaldClientError::WebSocket(tungstenite::Error::Http(response))
            if response.status() == StatusCode::NOT_FOUND
    )
}

fn runtime_spec(pane: &TerminalPane) -> TerminalRuntimeSpec {
    TerminalRuntimeSpec {
        cwd: pane.cwd.clone(),
        shell: pane.shell.clone(),
        arguments: pane.arguments.clone(),
        environment: Default::default(),
        rows: pane.rows,
        cols: pane.cols,
    }
}

fn apply_runtime(pane: &mut TerminalPane, runtime: &TerminalRuntime) -> bool {
    if pane.status == runtime.status
        && pane.exit_code == runtime.exit_code
        && pane.rows == runtime.rows
        && pane.cols == runtime.cols
    {
        return false;
    }
    pane.status = runtime.status;
    pane.exit_code = runtime.exit_code;
    pane.rows = runtime.rows;
    pane.cols = runtime.cols;
    pane.updated_at = timestamp();
    true
}

fn find_pane(tabs: &[TerminalTab], pane_id: &str) -> Option<(usize, usize)> {
    for (tab_index, tab) in tabs.iter().enumerate() {
        if let Some(index) = tab.panes.iter().position(|pane| pane.id == pane_id) {
            return Some((tab_index, index));
        }
    }
    None
}

fn resolve_cwd(cwd: Option<&str>, workspace_root: Option<&str>) -> Result<String, TerminalError> {
    let value = cwd
        .filter(|value| !value.trim().is_empty())
        .or_else(|| workspace_root.filter(|value| !value.trim().is_empty()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy()
                .into_owned()
        });
    if !Path::new(&value).is_absolute() {
        return Err(TerminalError::Invalid(format!(
            "terminal cwd must be absolute: {value}"
        )));
    }
    Ok(value)
}

async fn validate_directory(path: &str) -> Result<(), TerminalError> {
    if !Path::new(path).is_absolute() {
        return Err(TerminalError::Invalid(format!(
            "directory must be absolute: {path}"
        )));
    }
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| TerminalError::Invalid(format!("cannot access {path}: {error}")))?;
    if !metadata.is_dir() {
        return Err(TerminalError::Invalid(format!(
            "path is not a directory: {path}"
        )));
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, TerminalError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn create_private_directory(path: &Path, allow_existing: bool) -> std::io::Result<()> {
    let result = std::fs::DirBuilder::new()
        .recursive(allow_existing)
        .create(path);
    if let Err(error) = result
        && !(allow_existing && error.kind() == std::io::ErrorKind::AlreadyExists)
    {
        return Err(error);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

async fn open_private_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let file = options.open(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(file)
}

fn clipboard_image_type(bytes: &[u8]) -> Option<ClipboardImageType> {
    if valid_png(bytes) {
        Some(ClipboardImageType {
            mime: "image/png",
            extension: "png",
        })
    } else if valid_jpeg(bytes) {
        Some(ClipboardImageType {
            mime: "image/jpeg",
            extension: "jpg",
        })
    } else if valid_gif(bytes) {
        Some(ClipboardImageType {
            mime: "image/gif",
            extension: "gif",
        })
    } else if valid_webp(bytes) {
        Some(ClipboardImageType {
            mime: "image/webp",
            extension: "webp",
        })
    } else {
        None
    }
}

fn valid_png(bytes: &[u8]) -> bool {
    if bytes.len() < 45 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8;
    let mut saw_header = false;
    let mut saw_image_data = false;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = match offset
            .checked_add(12)
            .and_then(|base| base.checked_add(length))
        {
            Some(end) if end <= bytes.len() => end,
            _ => return false,
        };
        let kind = &bytes[offset + 4..offset + 8];
        if !saw_header {
            if kind != b"IHDR" || length != 13 {
                return false;
            }
            let width = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
            if width == 0 || height == 0 {
                return false;
            }
            saw_header = true;
        }
        if kind == b"IDAT" && length > 0 {
            saw_image_data = true;
        }
        if kind == b"IEND" {
            return saw_header && saw_image_data && length == 0 && end == bytes.len();
        }
        offset = end;
    }
    false
}

fn valid_jpeg(bytes: &[u8]) -> bool {
    if bytes.len() < 24 || !bytes.starts_with(b"\xff\xd8") || !bytes.ends_with(b"\xff\xd9") {
        return false;
    }
    let mut offset = 2;
    let mut saw_frame = false;
    while offset + 1 < bytes.len() - 2 {
        if bytes[offset] != 0xff {
            return false;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            return false;
        }
        let marker = bytes[offset];
        offset += 1;
        if marker == 0xda {
            return saw_frame;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if offset + 2 > bytes.len() - 2 {
            return false;
        }
        let length = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        if length < 2 || offset + length > bytes.len() - 2 {
            return false;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 8 {
                return false;
            }
            let height = u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().unwrap());
            let width = u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().unwrap());
            if width == 0 || height == 0 {
                return false;
            }
            saw_frame = true;
        }
        offset += length;
    }
    false
}

fn valid_gif(bytes: &[u8]) -> bool {
    bytes.len() >= 14
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && bytes[6..10].iter().any(|byte| *byte != 0)
        && bytes[10..bytes.len() - 1].contains(&0x2c)
        && bytes.last() == Some(&0x3b)
}

fn valid_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8;
    if declared != bytes.len() {
        return false;
    }

    let mut offset = 12;
    let mut saw_extended_header = false;
    let mut saw_extended_chunk = false;
    let mut saw_image_data = false;
    while offset + 8 <= bytes.len() {
        let kind = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_start = offset + 8;
        let Some(data_end) = data_start.checked_add(size) else {
            return false;
        };
        let Some(next) = data_end.checked_add(size & 1) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        match kind {
            b"VP8 " if size > 0 => saw_image_data = true,
            b"VP8L" if size >= 5 && bytes[data_start] == 0x2f => saw_image_data = true,
            b"VP8X" if size == 10 && !saw_extended_header => saw_extended_header = true,
            b"ALPH" if size > 0 => saw_extended_chunk = true,
            b"ANIM" if size == 6 => saw_extended_chunk = true,
            b"ANMF" if size >= 16 => {
                saw_extended_chunk = true;
                saw_image_data = true;
            }
            b"ICCP" | b"EXIF" | b"XMP " => saw_extended_chunk = true,
            b"VP8 " | b"VP8L" | b"VP8X" | b"ALPH" | b"ANIM" | b"ANMF" => return false,
            _ => {}
        }
        offset = next;
    }
    offset == bytes.len() && saw_image_data && (!saw_extended_chunk || saw_extended_header)
}

fn normalize_shell(shell: Option<&str>) -> Result<String, TerminalError> {
    let shell = shell
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var("SHELL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| {
            if Path::new("/bin/bash").is_file() {
                "/bin/bash".to_owned()
            } else {
                "/bin/sh".to_owned()
            }
        });
    if shell.as_bytes().contains(&0) {
        return Err(TerminalError::Invalid(
            "shell contains a NUL byte".to_owned(),
        ));
    }
    Ok(shell)
}

fn terminal_size(rows: Option<u16>, cols: Option<u16>) -> Result<(u16, u16), TerminalError> {
    let rows = rows.unwrap_or(DEFAULT_ROWS);
    let cols = cols.unwrap_or(DEFAULT_COLS);
    validate_dimensions(rows, cols)?;
    Ok((rows, cols))
}

fn validate_dimensions(rows: u16, cols: u16) -> Result<(), TerminalError> {
    if !(1..=1000).contains(&rows) || !(1..=1000).contains(&cols) {
        return Err(TerminalError::Invalid(
            "terminal rows and cols must be between 1 and 1000".to_owned(),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<String, TerminalError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TerminalError::Invalid(
            "terminal name cannot be empty".to_owned(),
        ));
    }
    if name.chars().count() > 128 {
        return Err(TerminalError::Invalid(
            "terminal name cannot exceed 128 characters".to_owned(),
        ));
    }
    Ok(name.to_owned())
}

fn default_pane_name(cwd: &str, shell: &str) -> String {
    let name = Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or_else(|| (!cwd.trim().is_empty()).then_some(cwd.trim()))
        .or_else(|| {
            Path::new(shell)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or("Shell");
    name.chars().take(128).collect()
}

fn migrate_terminal_names(tab: &mut TerminalTab) {
    // Old metadata did not distinguish generated names from explicit renames.
    // Recognize only the old defaults; preserve all other labels. New renames
    // carry an explicit flag, even when the user chooses a default-looking name.
    let is_agent_default = |name: &str, pane: &TerminalPane| {
        pane.agent_id
            .as_deref()
            .and_then(aow_agents::Agent::from_id)
            .is_some_and(|agent| agent.definition().display_name == name)
    };
    if tab.name_is_custom.is_none() {
        let numbered = tab
            .name
            .strip_prefix("Terminal ")
            .is_some_and(|suffix| suffix.parse::<usize>().is_ok());
        let generated = numbered
            || tab
                .panes
                .iter()
                .any(|pane| is_agent_default(&tab.name, pane));
        tab.name_is_custom = Some(!generated);
    }
}

fn validate_ratio(ratio: f32) -> Result<f32, TerminalError> {
    if !ratio.is_finite() || !(0.05..=0.95).contains(&ratio) {
        return Err(TerminalError::Invalid(
            "split ratio must be between 0.05 and 0.95".to_owned(),
        ));
    }
    Ok(ratio)
}

fn tab_index(state: &ManagerState, tab_id: &str) -> Result<usize, TerminalError> {
    state
        .tabs
        .iter()
        .position(|tab| tab.id == tab_id)
        .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))
}

fn touch_tab(tab: &mut TerminalTab) {
    tab.revision = tab.revision.saturating_add(1);
    tab.updated_at = timestamp();
}

fn replace_layout_leaf(
    layout: &mut TerminalLayout,
    pane_id: &str,
    replacement: &TerminalLayout,
) -> bool {
    match layout {
        TerminalLayout::Pane { pane_id: current } if current == pane_id => {
            *layout = replacement.clone();
            true
        }
        TerminalLayout::Pane { .. } => false,
        TerminalLayout::Split { first, second, .. } => {
            replace_layout_leaf(first, pane_id, replacement)
                || replace_layout_leaf(second, pane_id, replacement)
        }
    }
}

fn remove_layout_leaf(layout: TerminalLayout, pane_id: &str) -> Option<TerminalLayout> {
    match layout {
        TerminalLayout::Pane { pane_id: current } => {
            (current != pane_id).then_some(TerminalLayout::Pane { pane_id: current })
        }
        TerminalLayout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            if layout_contains(&first, pane_id) {
                match remove_layout_leaf(*first, pane_id) {
                    Some(first) => Some(TerminalLayout::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second,
                    }),
                    None => Some(*second),
                }
            } else {
                match remove_layout_leaf(*second, pane_id) {
                    Some(second) => Some(TerminalLayout::Split {
                        axis,
                        ratio,
                        first,
                        second: Box::new(second),
                    }),
                    None => Some(*first),
                }
            }
        }
    }
}

fn layout_contains(layout: &TerminalLayout, pane_id: &str) -> bool {
    match layout {
        TerminalLayout::Pane { pane_id: current } => current == pane_id,
        TerminalLayout::Split { first, second, .. } => {
            layout_contains(first, pane_id) || layout_contains(second, pane_id)
        }
    }
}

fn validate_layout(layout: &TerminalLayout, panes: &[TerminalPane]) -> Result<(), TerminalError> {
    let expected = panes
        .iter()
        .map(|pane| pane.id.as_str())
        .collect::<HashSet<_>>();
    let mut found = HashSet::new();
    collect_layout_panes(layout, 0, &mut found)?;
    if found != expected {
        return Err(TerminalError::Invalid(
            "layout must contain every pane exactly once".to_owned(),
        ));
    }
    Ok(())
}

fn collect_layout_panes<'a>(
    layout: &'a TerminalLayout,
    depth: usize,
    panes: &mut HashSet<&'a str>,
) -> Result<(), TerminalError> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(TerminalError::Invalid(format!(
            "layout exceeds maximum depth {MAX_LAYOUT_DEPTH}"
        )));
    }
    match layout {
        TerminalLayout::Pane { pane_id } => {
            if !panes.insert(pane_id) {
                return Err(TerminalError::Invalid(format!(
                    "pane appears more than once in layout: {pane_id}"
                )));
            }
        }
        TerminalLayout::Split {
            ratio,
            first,
            second,
            ..
        } => {
            validate_ratio(*ratio)?;
            collect_layout_panes(first, depth + 1, panes)?;
            collect_layout_panes(second, depth + 1, panes)?;
        }
    }
    Ok(())
}

fn validate_persisted_tabs(tabs: &[TerminalTab]) -> Result<(), TerminalError> {
    let mut tab_ids = HashSet::new();
    let mut pane_ids = HashSet::new();
    for tab in tabs {
        if !tab_ids.insert(tab.id.as_str()) {
            return Err(TerminalError::Invalid(format!(
                "duplicate terminal tab id: {}",
                tab.id
            )));
        }
        if tab.panes.is_empty() {
            return Err(TerminalError::Invalid(format!(
                "terminal tab has no panes: {}",
                tab.id
            )));
        }
        for pane in &tab.panes {
            if !pane_ids.insert(pane.id.as_str()) {
                return Err(TerminalError::Invalid(format!(
                    "duplicate terminal pane id: {}",
                    pane.id
                )));
            }
            validate_name(&pane.name)?;
        }
        validate_layout(&tab.layout, &tab.panes)?;
    }
    Ok(())
}

fn atomic_save(path: &Path, state: &PersistedState) -> Result<(), TerminalError> {
    let parent = path.parent().ok_or_else(|| {
        TerminalError::Invalid(format!("metadata path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(
        ".{METADATA_FILE}.{}.tmp",
        Uuid::new_v4().as_simple()
    ));
    let result = (|| {
        let bytes = serde_json::to_vec_pretty(state)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)?;
        Ok::<_, TerminalError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn terminal_http_error(error: TerminalError) -> HttpError {
    match error {
        TerminalError::TabNotFound(_) | TerminalError::PaneNotFound(_) => HttpError::new(
            StatusCode::NOT_FOUND,
            "terminal_not_found",
            error.to_string(),
            None,
        ),
        TerminalError::Invalid(_) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_terminal_request",
            error.to_string(),
            None,
        ),
        TerminalError::Conflict(_) => HttpError::new(
            StatusCode::CONFLICT,
            "terminal_conflict",
            error.to_string(),
            None,
        ),
        TerminalError::ForbiddenOrigin(_) => HttpError::new(
            StatusCode::FORBIDDEN,
            "forbidden_websocket_origin",
            error.to_string(),
            None,
        ),
        TerminalError::ClipboardImageTooLarge => HttpError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "clipboard_image_too_large",
            error.to_string(),
            None,
        ),
        TerminalError::UnsupportedClipboardImage => HttpError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_clipboard_image",
            error.to_string(),
            None,
        ),
        TerminalError::ClipboardQuotaExceeded => HttpError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "clipboard_image_quota_exceeded",
            error.to_string(),
            None,
        ),
        TerminalError::DaemonUnavailable(_) => HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminald_unavailable",
            error.to_string(),
            None,
        ),
        TerminalError::RuntimeCreate(_) => HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "terminal_spawn_failed",
            error.to_string(),
            None,
        ),
        TerminalError::Daemon(_)
        | TerminalError::Poisoned
        | TerminalError::ClipboardStorage(_)
        | TerminalError::Io(_)
        | TerminalError::Json(_) => HttpError::internal(error.to_string()),
    }
}

pub use aow_filesystem::default_state_dir;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use bytes::Bytes;
    use futures_util::Sink;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tempfile::TempDir;
    use tokio::sync::{mpsc, oneshot};

    fn minimal_png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.push(0);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn minimal_jpeg() -> Vec<u8> {
        b"\xff\xd8\xff\xc0\x00\x0b\x08\x00\x01\x00\x01\x01\x01\x11\x00\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x00\xff\xd9".to_vec()
    }

    fn minimal_gif() -> Vec<u8> {
        b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01L\x00;".to_vec()
    }

    fn minimal_webp() -> Vec<u8> {
        b"RIFF\x10\x00\x00\x00WEBPVP8 \x04\x00\x00\x00data".to_vec()
    }

    fn extended_webp() -> Vec<u8> {
        let mut bytes = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        bytes.extend_from_slice(b"VP8X\x0a\x00\x00\x00");
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(b"EXIF\x03\x00\x00\x00abc\x00");
        bytes.extend_from_slice(b"VP8 \x04\x00\x00\x00data");
        let riff_size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        bytes
    }
    use tower::ServiceExt;

    pub(super) fn pane(id: &str, status: TerminalPaneStatus) -> TerminalPane {
        TerminalPane {
            id: id.to_owned(),
            name: "tmp".to_owned(),
            cwd: "/tmp".to_owned(),
            shell: "/bin/sh".to_owned(),
            arguments: Vec::new(),
            kind: TerminalPaneKind::Terminal,
            agent_id: None,
            agent_profile_id: None,
            agent_terminal: None,
            restart_on_daemon_restart: true,
            status,
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            exit_code: None,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    pub(super) fn tab_with(layout: TerminalLayout, panes: Vec<TerminalPane>) -> TerminalTab {
        TerminalTab {
            id: "tab-1".to_owned(),
            name: "Terminal".to_owned(),
            name_is_custom: None,
            workspace_root: "/tmp".to_owned(),
            layout,
            panes,
            revision: 7,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    fn tab_for(id: &str, workspace_root: &str) -> TerminalTab {
        let pane_id = format!("{id}-pane");
        let mut tab = tab_with(
            TerminalLayout::Pane {
                pane_id: pane_id.clone(),
            },
            vec![pane(&pane_id, TerminalPaneStatus::Interrupted)],
        );
        tab.id = id.to_owned();
        tab.name = id.to_owned();
        tab.workspace_root = workspace_root.to_owned();
        tab
    }

    struct PendingSink;

    impl Sink<()> for PendingSink {
        type Error = std::convert::Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn start_send(self: Pin<&mut Self>, _item: ()) -> Result<(), Self::Error> {
            unreachable!("a permanently pending sink is never ready")
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn bridge_message_send_is_bounded() {
        let mut sink = PendingSink;
        let result = send_bridge_message(&mut sink, (), Duration::from_millis(1)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bridge_runtime_sync_is_bounded_while_waiting_for_sync_lock() {
        let manager = manager_with_pane();
        let _sync = manager.inner.runtime_sync.lock().await;
        let result = bounded_sync_runtime(&manager, "pane-1", Duration::from_millis(1)).await;
        assert!(result.is_err());
    }

    #[test]
    fn recursive_layout_replace_remove_and_validation() {
        let mut layout = TerminalLayout::Pane {
            pane_id: "a".into(),
        };
        let split = TerminalLayout::Split {
            axis: TerminalSplitAxis::Row,
            ratio: 0.4,
            first: Box::new(TerminalLayout::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(TerminalLayout::Pane {
                pane_id: "b".into(),
            }),
        };
        assert!(replace_layout_leaf(&mut layout, "a", &split));
        validate_layout(
            &layout,
            &[
                pane("a", TerminalPaneStatus::Running),
                pane("b", TerminalPaneStatus::Running),
            ],
        )
        .unwrap();
        assert_eq!(
            remove_layout_leaf(layout, "a"),
            Some(TerminalLayout::Pane {
                pane_id: "b".into()
            })
        );
    }

    #[test]
    fn persistent_load_preserves_running_desired_state() {
        let directory = TempDir::new().unwrap();
        let state_path = directory.path().join(METADATA_FILE);
        let mut original = tab_with(
            TerminalLayout::Pane {
                pane_id: "running".into(),
            },
            vec![pane("running", TerminalPaneStatus::Running)],
        );
        // Current metadata includes naming intent; legacy migration has its own test.
        original.name_is_custom = Some(true);
        atomic_save(
            &state_path,
            &PersistedState {
                version: METADATA_VERSION,
                tabs: vec![original.clone()],
            },
        )
        .unwrap();
        let manager = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        assert_eq!(manager.get_snapshot("tab-1").unwrap(), original);
        assert_eq!(
            std::fs::read(state_path).unwrap(),
            serde_json::to_vec_pretty(&PersistedState {
                version: METADATA_VERSION,
                tabs: vec![original]
            })
            .unwrap()
            .into_iter()
            .chain([b'\n'])
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn terminal_reorder_persists_within_workspace_and_preserves_other_workspaces() {
        let directory = TempDir::new().unwrap();
        let manager = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        manager.lock_state().unwrap().tabs = vec![
            tab_for("a", "/workspace/one"),
            tab_for("x", "/workspace/two"),
            tab_for("b", "/workspace/one"),
            tab_for("y", "/workspace/two"),
            tab_for("c", "/workspace/one"),
        ];
        manager
            .persist_locked(&manager.lock_state().unwrap())
            .unwrap();

        let reordered = manager
            .reorder(ReorderTerminalsRequest {
                workspace_root: "/workspace/one".to_owned(),
                tab_ids: vec!["c".to_owned(), "a".to_owned(), "b".to_owned()],
            })
            .unwrap();
        assert_eq!(
            reordered
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
        assert_eq!(
            manager
                .list_snapshot(None)
                .unwrap()
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "x", "a", "y", "b"]
        );

        let reloaded = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        assert_eq!(
            reloaded
                .list_snapshot(Some("/workspace/one"))
                .unwrap()
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
        assert_eq!(
            reloaded
                .list_snapshot(Some("/workspace/two"))
                .unwrap()
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "y"]
        );
    }

    #[tokio::test]
    async fn delete_workspace_stops_runtimes_and_removes_only_matching_metadata() {
        let directory = TempDir::new().unwrap();
        let socket = directory.path().join("terminald").join("terminald.sock");
        let state_dir = directory.path().join("state");
        let target_root = directory.path().join("target");
        let other_root = directory.path().join("other");
        std::fs::create_dir_all(&target_root).unwrap();
        std::fs::create_dir_all(&other_root).unwrap();
        let target_root = target_root.to_string_lossy().into_owned();
        let other_root = other_root.to_string_lossy().into_owned();
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket.clone());
        let manager = TerminalManager::persistent(state_dir.clone(), client.clone()).unwrap();
        let terminal = manager
            .create(CreateTerminalRequest {
                name: Some("target terminal".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some(target_root.clone()),
                shell: Some("/bin/sh".to_owned()),
                agent_id: None,
                resume_session_id: None,
                rows: Some(24),
                cols: Some(80),
            })
            .await
            .unwrap();
        let agent = manager
            .create_agent(
                CreateTerminalRequest {
                    name: Some("target agent".to_owned()),
                    cwd: Some("/tmp".to_owned()),
                    workspace_root: Some(target_root.clone()),
                    shell: None,
                    agent_id: Some("test-agent".to_owned()),
                    resume_session_id: None,
                    rows: Some(24),
                    cols: Some(80),
                },
                AgentLaunch {
                    agent_type: aow_agents::launch::AgentType::Codex,
                    display_name: "Test Agent".to_owned(),
                    executable: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                    env: Default::default(),
                },
            )
            .await
            .unwrap();
        let other = manager
            .create(CreateTerminalRequest {
                name: Some("other terminal".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some(other_root.clone()),
                shell: Some("/bin/sh".to_owned()),
                agent_id: None,
                resume_session_id: None,
                rows: Some(24),
                cols: Some(80),
            })
            .await
            .unwrap();

        assert_eq!(manager.workspace_tab_counts(&target_root).unwrap(), (1, 1));
        assert_eq!(
            manager.delete_workspace(&target_root).await.unwrap(),
            (1, 1)
        );
        assert!(
            manager
                .list_snapshot(Some(&target_root))
                .unwrap()
                .tabs
                .is_empty()
        );
        assert_eq!(
            manager.list_snapshot(Some(&other_root)).unwrap().tabs.len(),
            1
        );
        for pane in terminal.panes.iter().chain(&agent.panes) {
            assert!(client.get(&pane.id).await.unwrap().is_none());
        }
        assert!(client.get(&other.panes[0].id).await.unwrap().is_some());

        let reloaded = TerminalManager::persistent(state_dir, client).unwrap();
        assert!(
            reloaded
                .list_snapshot(Some(&target_root))
                .unwrap()
                .tabs
                .is_empty()
        );
        assert_eq!(
            reloaded
                .list_snapshot(Some(&other_root))
                .unwrap()
                .tabs
                .len(),
            1
        );
        stop_daemon(shutdown, daemon).await;
    }

    #[tokio::test]
    async fn agent_pane_can_split_into_a_terminal_pane() {
        let directory = TempDir::new().unwrap();
        let socket = directory.path().join("terminald").join("terminald.sock");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket);
        let manager = TerminalManager::in_memory(client.clone());
        let tab = manager
            .create_agent(
                CreateTerminalRequest {
                    name: Some("Test Agent".to_owned()),
                    cwd: Some("/tmp".to_owned()),
                    workspace_root: Some("/tmp".to_owned()),
                    shell: None,
                    agent_id: Some("test-agent".to_owned()),
                    resume_session_id: None,
                    rows: Some(24),
                    cols: Some(80),
                },
                AgentLaunch {
                    agent_type: aow_agents::launch::AgentType::Codex,
                    display_name: "Test Agent".to_owned(),
                    executable: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                    env: Default::default(),
                },
            )
            .await
            .unwrap();
        let agent_pane_id = tab.panes[0].id.clone();
        assert_eq!(tab.panes[0].agent_id.as_deref(), Some("codex"));

        let updated = manager
            .split(
                &tab.id,
                SplitTerminalRequest {
                    target_pane_id: agent_pane_id.clone(),
                    axis: TerminalSplitAxis::Row,
                    ratio: None,
                    cwd: None,
                    shell: None,
                    rows: None,
                    cols: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.panes.len(), 2);
        let terminal = updated
            .panes
            .iter()
            .find(|pane| pane.id != agent_pane_id)
            .unwrap();
        assert_eq!(terminal.kind, TerminalPaneKind::Terminal);
        assert_eq!(terminal.agent_id, None);
        assert_eq!(terminal.shell, normalize_shell(None).unwrap());
        assert!(terminal.arguments.is_empty());
        assert!(matches!(
            updated.layout,
            TerminalLayout::Split {
                axis: TerminalSplitAxis::Row,
                ..
            }
        ));
        assert!(client.get(&agent_pane_id).await.unwrap().is_some());
        assert!(client.get(&terminal.id).await.unwrap().is_some());

        stop_daemon(shutdown, daemon).await;
    }

    #[test]
    fn terminal_reorder_rejects_duplicate_or_stale_ids_without_mutating_state() {
        let manager = TerminalManager::in_memory(TerminaldClient::new(
            std::env::temp_dir().join("missing-terminald.sock"),
        ));
        manager.lock_state().unwrap().tabs =
            vec![tab_for("a", "/workspace"), tab_for("b", "/workspace")];

        assert!(matches!(
            manager.reorder(ReorderTerminalsRequest {
                workspace_root: "/workspace".to_owned(),
                tab_ids: vec!["a".to_owned(), "a".to_owned()],
            }),
            Err(TerminalError::Invalid(_))
        ));
        assert!(matches!(
            manager.reorder(ReorderTerminalsRequest {
                workspace_root: "/workspace".to_owned(),
                tab_ids: vec!["b".to_owned()],
            }),
            Err(TerminalError::Conflict(_))
        ));
        assert_eq!(
            manager
                .list_snapshot(Some("/workspace"))
                .unwrap()
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[tokio::test]
    async fn terminal_reorder_route_returns_the_persisted_workspace_order() {
        let manager = TerminalManager::in_memory(TerminaldClient::new(
            std::env::temp_dir().join("missing-terminald.sock"),
        ));
        manager.lock_state().unwrap().tabs =
            vec![tab_for("a", "/workspace"), tab_for("b", "/workspace")];
        let response = clipboard_app(manager)
            .oneshot(
                Request::put("/api/terminals/order")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "workspace_root": "/workspace",
                            "tab_ids": ["b", "a"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = response_json(response).await;
        assert_eq!(response["tabs"][0]["id"], "b");
        assert_eq!(response["tabs"][1]["id"], "a");
    }

    #[tokio::test]
    async fn terminal_name_intent_migrates_and_survives_explicit_rename() {
        let directory = TempDir::new().unwrap();
        let client = TerminaldClient::new(directory.path().join("missing.sock"));
        let manager =
            TerminalManager::persistent(directory.path().to_path_buf(), client.clone()).unwrap();
        let mut generated = tab_for("auto", "/workspace");
        generated.name = "Terminal 1".to_owned();
        let mut custom = tab_for("custom", "/workspace");
        custom.name = "My shell".to_owned();
        let mut agent = tab_for("agent", "/workspace");
        agent.name = "TraeCode CLI".to_owned();
        agent.panes[0].agent_id = Some("traecli".to_owned());
        agent.panes[0].name = "TraeCode CLI".to_owned();
        let mut cli = agent.clone();
        cli.id = "cli".to_owned();
        cli.panes[0].id = "cli-pane".to_owned();
        cli.layout = TerminalLayout::Pane {
            pane_id: "cli-pane".to_owned(),
        };
        cli.panes[0].agent_terminal = Some(aow_protocol::AgentTerminalState {
            phase: aow_protocol::AgentTerminalPhase::Ready,
            error: None,
            task_submitted: false,
        });
        manager.lock_state().unwrap().tabs = vec![generated, custom, agent, cli];
        manager
            .persist_locked(&manager.lock_state().unwrap())
            .unwrap();
        let manager =
            TerminalManager::persistent(directory.path().to_path_buf(), client.clone()).unwrap();
        assert_eq!(
            manager.get_snapshot("auto").unwrap().name_is_custom,
            Some(false)
        );
        assert_eq!(
            manager.get_snapshot("custom").unwrap().name_is_custom,
            Some(true)
        );
        let agent = manager.get_snapshot("agent").unwrap();
        assert_eq!(agent.name_is_custom, Some(false));
        let app = clipboard_app(manager.clone());
        for phase in [
            aow_protocol::AgentTerminalPhase::Starting,
            aow_protocol::AgentTerminalPhase::Ready,
            aow_protocol::AgentTerminalPhase::Failed,
        ] {
            manager
                .lock_state()
                .unwrap()
                .tabs
                .iter_mut()
                .find(|tab| tab.id == "cli")
                .unwrap()
                .panes[0]
                .agent_terminal
                .as_mut()
                .unwrap()
                .phase = phase;
            let before = manager.get_snapshot("cli").unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::patch("/api/terminals/cli")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"name":"custom CLI name"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(manager.get_snapshot("cli").unwrap(), before);
        }
        manager
            .update(
                "agent",
                UpdateTerminalRequest {
                    name: Some("User agent".to_owned()),
                },
            )
            .unwrap();
        assert_eq!(manager.get_snapshot("agent").unwrap().name, "User agent");
        manager
            .update(
                "auto",
                UpdateTerminalRequest {
                    name: Some("Terminal 1".to_owned()),
                },
            )
            .unwrap();
        let manager = TerminalManager::persistent(directory.path().to_path_buf(), client).unwrap();
        let tab = manager.get_snapshot("auto").unwrap();
        assert_eq!(tab.name_is_custom, Some(true));
    }

    #[tokio::test]
    async fn retired_pane_rename_route_does_not_change_metadata() {
        let directory = TempDir::new().unwrap();
        let manager = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        manager
            .lock_state()
            .unwrap()
            .tabs
            .push(tab_for("tab-1", "/workspace"));
        manager
            .persist_locked(&manager.lock_state().unwrap())
            .unwrap();

        let response = clipboard_app(manager)
            .oneshot(
                Request::patch("/api/terminals/tab-1/panes/tab-1-pane")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"API server"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        let reloaded = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        let pane = &reloaded.get_snapshot("tab-1").unwrap().panes[0];
        assert_eq!(pane.name, "tmp");
        assert_eq!(pane.cwd, "/tmp");
    }

    #[test]
    fn persistent_load_gives_legacy_panes_a_directory_name() {
        let directory = TempDir::new().unwrap();
        let state_path = directory.path().join(METADATA_FILE);
        let state = serde_json::json!({
            "version": METADATA_VERSION,
            "tabs": [{
                "id": "tab-1",
                "name": "Terminal",
                "workspace_root": "/workspace",
                "layout": { "type": "pane", "pane_id": "pane-1" },
                "panes": [{
                    "id": "pane-1",
                    "cwd": "/workspace/project",
                    "shell": "/bin/bash",
                    "status": "interrupted",
                    "rows": 24,
                    "cols": 80,
                    "created_at": "2026-01-01T00:00:00.000Z",
                    "updated_at": "2026-01-01T00:00:00.000Z"
                }],
                "revision": 1,
                "created_at": "2026-01-01T00:00:00.000Z",
                "updated_at": "2026-01-01T00:00:00.000Z"
            }]
        });
        std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

        let manager = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();

        assert_eq!(
            manager.get_snapshot("tab-1").unwrap().panes[0].name,
            "project"
        );
    }

    #[test]
    fn legacy_pane_custom_names_are_discarded_while_tab_names_survive() {
        let directory = TempDir::new().unwrap();
        let state_path = directory.path().join(METADATA_FILE);
        let mut tab = serde_json::to_value(tab_for("tab", "/workspace")).unwrap();
        tab["name"] = serde_json::json!("My tab");
        tab["name_is_custom"] = serde_json::json!(true);
        tab["panes"][0]["name"] = serde_json::json!("Obsolete pane label");
        for flag in [
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::Value::Null,
        ] {
            tab["panes"][0]["name_is_custom"] = flag;
            std::fs::write(
                &state_path,
                serde_json::to_vec(&serde_json::json!({
                    "version": METADATA_VERSION, "tabs": [tab.clone()]
                }))
                .unwrap(),
            )
            .unwrap();
            let manager = TerminalManager::persistent(
                directory.path().to_path_buf(),
                TerminaldClient::new(directory.path().join("missing.sock")),
            )
            .unwrap();
            let loaded = manager.get_snapshot("tab").unwrap();
            assert_eq!(loaded.name, "My tab");
            assert_eq!(loaded.name_is_custom, Some(true));
            assert_eq!(loaded.panes[0].name, "tmp");
            assert_eq!(loaded.panes[0].cwd, "/tmp");
            assert!(
                serde_json::to_value(&loaded).unwrap()["panes"][0]
                    .get("name_is_custom")
                    .is_none()
            );
        }
    }

    #[test]
    fn layout_revision_conflicts_are_rejected() {
        let manager = TerminalManager::in_memory(TerminaldClient::new("/tmp/missing.sock".into()));
        let tab = tab_with(
            TerminalLayout::Pane {
                pane_id: "a".into(),
            },
            vec![pane("a", TerminalPaneStatus::Interrupted)],
        );
        manager.lock_state().unwrap().tabs.push(tab.clone());
        assert!(matches!(
            manager.update_layout(
                &tab.id,
                UpdateLayoutRequest {
                    layout: tab.layout.clone(),
                    revision: Some(tab.revision - 1),
                }
            ),
            Err(TerminalError::Conflict(_))
        ));
    }

    #[test]
    fn split_rollback_preserves_concurrent_tab_and_runtime_updates() {
        let manager = TerminalManager::in_memory(TerminaldClient::new("/tmp/missing.sock".into()));
        let previous = tab_with(
            TerminalLayout::Pane {
                pane_id: "a".into(),
            },
            vec![pane("a", TerminalPaneStatus::Running)],
        );
        let mut created = previous.clone();
        created.layout = TerminalLayout::Split {
            axis: TerminalSplitAxis::Row,
            ratio: 0.5,
            first: Box::new(TerminalLayout::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(TerminalLayout::Pane {
                pane_id: "failed".into(),
            }),
        };
        created
            .panes
            .push(pane("failed", TerminalPaneStatus::Running));
        created.revision += 1;

        let mut current = created.clone();
        current.name = "renamed concurrently".to_owned();
        current.revision += 1;
        current.panes[0].status = TerminalPaneStatus::Exited;
        current.panes[0].exit_code = Some(9);
        manager.lock_state().unwrap().tabs.push(current);
        manager
            .rollback_split(&created.id, "failed", &created, previous)
            .unwrap();

        let rolled_back = manager.get_snapshot(&created.id).unwrap();
        assert_eq!(rolled_back.name, "renamed concurrently");
        assert_eq!(rolled_back.panes.len(), 1);
        assert_eq!(rolled_back.panes[0].id, "a");
        assert_eq!(rolled_back.panes[0].status, TerminalPaneStatus::Exited);
        assert_eq!(rolled_back.panes[0].exit_code, Some(9));
        assert_eq!(
            rolled_back.layout,
            TerminalLayout::Pane {
                pane_id: "a".into()
            }
        );
    }

    #[test]
    fn terminal_request_origin_must_match_request_host() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "workspace.example:8080".parse().unwrap());
        assert!(validate_request_origin(&headers).is_ok());
        headers.insert(ORIGIN, "https://workspace.example:8080".parse().unwrap());
        assert!(validate_request_origin(&headers).is_ok());
        headers.insert(ORIGIN, "http://evil.example:8080".parse().unwrap());
        assert!(matches!(
            validate_request_origin(&headers),
            Err(TerminalError::ForbiddenOrigin(_))
        ));
    }

    #[test]
    fn clipboard_magic_identifies_supported_formats() {
        assert_eq!(
            clipboard_image_type(&minimal_png()).unwrap().mime,
            "image/png"
        );
        assert_eq!(
            clipboard_image_type(&minimal_jpeg()).unwrap().mime,
            "image/jpeg"
        );
        assert_eq!(
            clipboard_image_type(&minimal_gif()).unwrap().mime,
            "image/gif"
        );
        assert_eq!(
            clipboard_image_type(&minimal_webp()).unwrap().mime,
            "image/webp"
        );
        assert_eq!(
            clipboard_image_type(&extended_webp()).unwrap().mime,
            "image/webp"
        );
        assert!(clipboard_image_type(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(clipboard_image_type(b"<svg").is_none());
    }

    fn manager_with_pane() -> TerminalManager {
        let manager = TerminalManager::in_memory(TerminaldClient::new(
            std::env::temp_dir().join("missing-terminald.sock"),
        ));
        manager.lock_state().unwrap().tabs.push(tab_with(
            TerminalLayout::Pane {
                pane_id: "pane-1".to_owned(),
            },
            vec![pane("pane-1", TerminalPaneStatus::Running)],
        ));
        manager
    }

    fn clipboard_app(manager: TerminalManager) -> Router {
        routes().with_state(AppState {
            base_path: crate::BasePath::default(),
            frontend_dist: PathBuf::new(),
            auth: crate::auth::PinAuth::disabled(),
            session_shares: crate::session_shares::SessionShares::in_memory(),
            terminals: manager,
            aow: crate::aow::AowManager::in_memory(),
            automations: None,
            operations: crate::operations::OperationService::in_memory(),
            review_providers: crate::pull_requests::ProviderManager::default(),
        })
    }

    fn clipboard_request(pane_id: &str, body: Body) -> Request<Body> {
        Request::post(format!(
            "/api/terminals/tab-1/panes/{pane_id}/clipboard-images"
        ))
        .header(HOST, "workspace.example:8080")
        .header(ORIGIN, "https://workspace.example:8080")
        .body(body)
        .unwrap()
    }

    async fn response_json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn clipboard_image_route_stores_private_png() {
        let manager = manager_with_pane();
        let directory = manager.inner.clipboard.directory.clone();
        let image = minimal_png();
        let response = clipboard_app(manager.clone())
            .oneshot(clipboard_request("pane-1", Body::from(image.clone())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let response = response_json(response).await;
        assert_eq!(response["mime"], "image/png");
        assert_eq!(response["size"], image.len());
        assert!(response["expires_at"].as_str().unwrap().ends_with('Z'));

        let path = PathBuf::from(response["path"].as_str().unwrap());
        assert!(path.is_absolute());
        assert_eq!(path.parent(), Some(directory.as_path()));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        Uuid::parse_str(path.file_stem().unwrap().to_str().unwrap()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), image);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn clipboard_image_route_rejects_unsupported_missing_pane_and_origin() {
        let manager = manager_with_pane();
        let app = clipboard_app(manager);

        let unsupported = app
            .clone()
            .oneshot(clipboard_request(
                "pane-1",
                Body::from("<svg xmlns='http://www.w3.org/2000/svg'/>"),
            ))
            .await
            .unwrap();
        assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let missing = app
            .clone()
            .oneshot(clipboard_request(
                "missing",
                Body::from(&b"\x89PNG\r\n\x1a\n"[..]),
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let mut wrong_origin = clipboard_request("pane-1", Body::from(&b"\x89PNG\r\n\x1a\n"[..]));
        wrong_origin
            .headers_mut()
            .insert(ORIGIN, "https://evil.example:8080".parse().unwrap());
        let forbidden = app.oneshot(wrong_origin).await.unwrap();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn clipboard_image_route_preflights_content_length_limit() {
        let manager = manager_with_pane();
        let mut request = clipboard_request("pane-1", Body::empty());
        request.headers_mut().insert(
            CONTENT_LENGTH,
            (MAX_CLIPBOARD_IMAGE_BYTES + 1).to_string().parse().unwrap(),
        );
        let response = clipboard_app(manager).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn clipboard_image_stream_enforces_limit_and_removes_temp_file() {
        let manager = manager_with_pane();
        let result = manager
            .store_clipboard_image(
                "tab-1",
                "pane-1",
                Body::from(vec![0_u8; MAX_CLIPBOARD_IMAGE_BYTES as usize + 1]),
            )
            .await;
        assert!(matches!(result, Err(TerminalError::ClipboardImageTooLarge)));
        assert_eq!(
            std::fs::read_dir(&manager.inner.clipboard.directory)
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn clipboard_image_upload_rejects_a_pane_deleted_before_commit() {
        let manager = manager_with_pane();
        let image = minimal_png();
        let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|item| (item, receiver))
        });
        let upload_manager = manager.clone();
        let upload = tokio::spawn(async move {
            upload_manager
                .store_clipboard_image("tab-1", "pane-1", Body::from_stream(stream))
                .await
        });
        sender
            .send(Ok(Bytes::copy_from_slice(&image[..20])))
            .await
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            if std::fs::read_dir(&manager.inner.clipboard.directory)
                .unwrap()
                .next()
                .is_some()
            {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::task::yield_now().await;
        }
        manager.lock_state().unwrap().tabs.clear();
        sender
            .send(Ok(Bytes::copy_from_slice(&image[20..])))
            .await
            .unwrap();
        drop(sender);
        let result = upload.await.unwrap();
        assert!(matches!(result, Err(TerminalError::TabNotFound(_))));
        assert_eq!(
            std::fs::read_dir(&manager.inner.clipboard.directory)
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn clipboard_image_upload_enforces_directory_quota() {
        let manager = manager_with_pane();
        let existing = manager.inner.clipboard.directory.join("existing.png");
        let file = std::fs::File::create(existing).unwrap();
        file.set_len(MAX_CLIPBOARD_DIRECTORY_BYTES).unwrap();

        let result = manager
            .store_clipboard_image("tab-1", "pane-1", Body::from(&b"\x89PNG\r\n\x1a\n"[..]))
            .await;
        assert!(matches!(result, Err(TerminalError::ClipboardQuotaExceeded)));
    }

    #[test]
    fn clipboard_cleanup_removes_expired_files() {
        let manager = manager_with_pane();
        let directory = &manager.inner.clipboard.directory;
        let expired = directory.join("expired.png");
        let live = directory.join("live.png");
        std::fs::write(&expired, b"expired").unwrap();
        std::fs::write(&live, b"live").unwrap();
        let now = SystemTime::now();
        std::fs::File::options()
            .write(true)
            .open(&expired)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(now - CLIPBOARD_IMAGE_TTL - Duration::from_secs(1)),
            )
            .unwrap();

        let used = manager.inner.clipboard.clean_expired_at(now).unwrap();
        assert!(!expired.exists());
        assert!(live.exists());
        assert_eq!(used, 4);
    }

    #[test]
    fn persistent_manager_uses_absolute_directory_and_cleans_on_creation() {
        let root = TempDir::new().unwrap();
        let state_dir = root.path().join("state");
        let directory = state_dir.join(CLIPBOARD_DIRECTORY);
        std::fs::create_dir_all(&directory).unwrap();
        let expired = directory.join("expired.png");
        std::fs::write(&expired, b"expired").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&expired)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - CLIPBOARD_IMAGE_TTL - Duration::from_secs(1)),
            )
            .unwrap();

        let manager = TerminalManager::persistent(
            state_dir,
            TerminaldClient::new(root.path().join("missing.sock")),
        )
        .unwrap();
        assert!(manager.inner.clipboard.directory.is_absolute());
        assert_eq!(
            manager.inner.clipboard.directory,
            std::fs::canonicalize(directory).unwrap()
        );
        assert!(!expired.exists());
    }

    #[tokio::test]
    async fn failed_create_and_split_roll_back_desired_state() {
        let directory = TempDir::new().unwrap();
        let manager =
            TerminalManager::in_memory(TerminaldClient::new(directory.path().join("missing.sock")));
        let create = manager
            .create(CreateTerminalRequest {
                name: Some("rollback".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some("/tmp".to_owned()),
                shell: Some("/bin/sh".to_owned()),
                agent_id: None,
                resume_session_id: None,
                rows: None,
                cols: None,
            })
            .await;
        assert!(matches!(create, Err(TerminalError::DaemonUnavailable(_))));
        assert!(manager.list_snapshot(None).unwrap().tabs.is_empty());

        let original = tab_with(
            TerminalLayout::Pane {
                pane_id: "existing".into(),
            },
            vec![pane("existing", TerminalPaneStatus::Running)],
        );
        manager.lock_state().unwrap().tabs.push(original.clone());
        let split = manager
            .split(
                &original.id,
                SplitTerminalRequest {
                    target_pane_id: "existing".to_owned(),
                    axis: TerminalSplitAxis::Row,
                    ratio: None,
                    cwd: None,
                    shell: None,
                    rows: None,
                    cols: None,
                },
            )
            .await;
        assert!(matches!(split, Err(TerminalError::DaemonUnavailable(_))));
        assert_eq!(manager.get_snapshot(&original.id).unwrap(), original);
    }

    #[tokio::test]
    async fn reconcile_cleans_orphans_rebuilds_after_daemon_restart_and_syncs_exit() {
        let directory = TempDir::new().unwrap();
        let socket = directory.path().join("terminald").join("terminald.sock");
        let state_dir = directory.path().join("state");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket.clone());
        let manager = TerminalManager::persistent(state_dir.clone(), client.clone()).unwrap();
        let tab = manager
            .create(CreateTerminalRequest {
                name: Some("reconcile".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some("/tmp".to_owned()),
                shell: Some("/bin/sh".to_owned()),
                agent_id: None,
                resume_session_id: None,
                rows: Some(24),
                cols: Some(80),
            })
            .await
            .unwrap();
        let pane_id = tab.panes[0].id.clone();
        client
            .create(
                "orphan",
                &TerminalRuntimeSpec {
                    cwd: "/tmp".to_owned(),
                    shell: "/bin/sh".to_owned(),
                    arguments: Vec::new(),
                    environment: Default::default(),
                    rows: 24,
                    cols: 80,
                },
            )
            .await
            .unwrap();
        manager.reconcile().await.unwrap();
        assert!(client.get("orphan").await.unwrap().is_none());

        let first_instance = client.health().await.unwrap().instance_id;
        stop_daemon(shutdown, daemon).await;
        assert_eq!(
            manager.get_snapshot(&tab.id).unwrap().panes[0].status,
            TerminalPaneStatus::Running
        );

        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let second_instance = client.health().await.unwrap().instance_id;
        assert_ne!(first_instance, second_instance);
        manager.reconcile().await.unwrap();
        assert_eq!(
            client.get(&pane_id).await.unwrap().unwrap().status,
            TerminalPaneStatus::Running
        );

        let mut attachment = client.attach(&pane_id).await.unwrap();
        attachment
            .send(tungstenite::Message::Binary(bytes::Bytes::from_static(
                b"exit 23\n",
            )))
            .await
            .unwrap();
        drop(attachment);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let runtime = client.get(&pane_id).await.unwrap().unwrap();
            if runtime.status == TerminalPaneStatus::Exited {
                assert_eq!(runtime.exit_code, Some(23));
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let updated = manager.get(&tab.id).await.unwrap();
        assert_eq!(updated.revision, tab.revision);
        assert_eq!(updated.panes[0].status, TerminalPaneStatus::Exited);
        assert_eq!(updated.panes[0].exit_code, Some(23));
        stop_daemon(shutdown, daemon).await;

        // Persisted panes remain desired state even after a natural exit. A
        // new daemon instance recreates every missing pane as a fresh shell.
        assert_eq!(
            manager.get_snapshot(&tab.id).unwrap().panes[0].status,
            TerminalPaneStatus::Exited
        );
        let (shutdown, daemon) = start_daemon(socket).await;
        manager.reconcile().await.unwrap();
        assert_eq!(
            manager.get_snapshot(&tab.id).unwrap().panes[0].status,
            TerminalPaneStatus::Running
        );
        stop_daemon(shutdown, daemon).await;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn terminal_agent_route_detects_start_exit_and_filters_workspaces() {
        let directory = TempDir::new().unwrap();
        let socket = directory.path().join("terminald/terminald.sock");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket);
        let manager = TerminalManager::in_memory(client.clone());
        let mut tabs = Vec::new();
        for name in ["a", "b"] {
            let workspace = directory.path().join(name);
            std::fs::create_dir(&workspace).unwrap();
            tabs.push(
                manager
                    .create(CreateTerminalRequest {
                        name: Some("Shell".to_owned()),
                        cwd: Some(directory.path().to_string_lossy().into_owned()),
                        workspace_root: Some(workspace.to_string_lossy().into_owned()),
                        shell: Some("/bin/sh".to_owned()),
                        agent_id: None,
                        resume_session_id: None,
                        rows: None,
                        cols: None,
                    })
                    .await
                    .unwrap(),
            );
        }
        let pane_id = &tabs[0].panes[0].id;
        let mut stream = client.attach(pane_id).await.unwrap();
        let mut other = client.attach(&tabs[1].panes[0].id).await.unwrap();
        other
            .send(tungstenite::Message::Binary(
                b"printf '\\033]2;Other workspace\\007'\n".to_vec().into(),
            ))
            .await
            .unwrap();
        drop(other);
        let app = clipboard_app(manager.clone());
        for (command, target, identity) in [
            ("codex", "codex", "codex"),
            ("claude", "claude", "claude"),
            ("traecli", "traecli", "traecli"),
            (".local/bin/traecli", "arbitrary-location/worker", "traecli"),
            ("custom/bin/traecli", "new-layout/renamed-worker", "traecli"),
        ] {
            let executable = directory.path().join(target);
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            let fixture = if cfg!(target_os = "macos") {
                std::env::current_exe().unwrap()
            } else {
                PathBuf::from("/bin/sh")
            };
            std::fs::copy(&fixture, &executable).unwrap();
            let entrypoint = directory.path().join(command);
            if entrypoint != executable {
                std::fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(&executable, &entrypoint).unwrap();
            }
            let command = if cfg!(target_os = "macos") {
                format!(
                    "AOW_TEST_AGENT_ID={identity} '{}' --ignored --exact terminal::tests::native_agent_route_fixture --nocapture\n",
                    entrypoint.display()
                )
            } else {
                format!(
                    "'{}' -c 'printf \"\\033]0;Session {identity}\\007\"; while IFS= read -r title; do printf \"\\033]2;%s\\007\" \"$title\"; done'\n",
                    entrypoint.display()
                )
            };
            stream
                .send(tungstenite::Message::Binary(command.into_bytes().into()))
                .await
                .unwrap();
            for expected in [Some(identity), None] {
                if expected.is_none() {
                    stream
                        .send(tungstenite::Message::Binary(vec![3].into()))
                        .await
                        .unwrap();
                }
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let response = app
                        .clone()
                        .oneshot(
                            Request::get(format!(
                                "/api/terminals/agents?workspace_root={}/a",
                                directory.path().display()
                            ))
                            .body(Body::empty())
                            .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    let body = response_json(response).await;
                    assert_eq!(body["agents"].as_object().unwrap().len(), 1);
                    assert!(body["titles"].get(&tabs[1].panes[0].id).is_none());
                    assert!(body["processes"].get(&tabs[1].panes[0].id).is_none());
                    if body["agents"][pane_id].as_str() == expected
                        && (expected.is_none()
                            || body["titles"][pane_id] == format!("Session {identity}"))
                    {
                        break;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "expected {expected:?}: {body}"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                let all = manager.agents(None).await.unwrap();
                assert_eq!(all.agents.get(&tabs[1].panes[0].id), Some(&None));
                if expected.is_some() {
                    let process = all.processes.get(pane_id).unwrap();
                    assert!(process.pid > 0);
                    assert!(!process.start_time.is_empty());
                    assert_eq!(
                        process.cwd,
                        directory.path().canonicalize().unwrap().to_string_lossy()
                    );
                    if entrypoint != executable {
                        // An update changes the symlink target while the old
                        // process retains its public launch entrypoint.
                        let next_link = entrypoint.with_extension("next");
                        std::os::unix::fs::symlink(&fixture, &next_link).unwrap();
                        std::fs::rename(next_link, &entrypoint).unwrap();
                        tokio::time::sleep(Duration::from_millis(1100)).await;
                        let updated = manager.agents(None).await.unwrap();
                        assert_eq!(updated.agents[pane_id].as_deref(), Some(identity));
                        assert_eq!(updated.processes[pane_id].pid, process.pid);
                    }
                    stream
                        .send(tungstenite::Message::Binary(
                            b"Updated title\n".to_vec().into(),
                        ))
                        .await
                        .unwrap();
                    // No browser attachment is needed to capture or retain titles.
                    drop(stream);
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                    loop {
                        if manager
                            .agents(None)
                            .await
                            .unwrap()
                            .titles
                            .get(pane_id)
                            .map(String::as_str)
                            == Some("Updated title")
                        {
                            break;
                        }
                        assert!(tokio::time::Instant::now() < deadline);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    stream = client.attach(pane_id).await.unwrap();
                }
            }
        }
        assert_eq!(manager.get_snapshot(&tabs[0].id).unwrap(), tabs[0]);
        drop(stream);
        stop_daemon(shutdown, daemon).await;
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "subprocess fixture invoked by the native Agent route test"]
    fn native_agent_route_fixture() {
        use std::io::{BufRead, Write};
        let Ok(agent) = std::env::var("AOW_TEST_AGENT_ID") else {
            return;
        };
        let mut stdout = std::io::stdout().lock();
        write!(stdout, "\x1b]0;Session {agent}\x07").unwrap();
        stdout.flush().unwrap();
        for line in std::io::stdin().lock().lines() {
            write!(stdout, "\x1b]2;{}\x07", line.unwrap()).unwrap();
            stdout.flush().unwrap();
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn pane_sessions_use_live_cwd_and_config_and_cache_readable_claude_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        let directory = TempDir::new().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let cwd = root.join("nested");
        let config = root.join("custom-claude");
        let bin = root.join("bin");
        let executable = root.join(".local/share/claude/versions/test");
        for path in [
            &cwd,
            &config.join("projects/demo"),
            &bin,
            executable.parent().unwrap(),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        // macOS redacts the environment of SIP-protected /bin programs. Use
        // our own harmless fixture executable there, like an ordinary CLI.
        let (fixture, arguments) = if cfg!(target_os = "macos") {
            (
                std::env::current_exe().unwrap(),
                "--ignored --exact terminal::sessions::tests::native_environment_fixture",
            )
        } else {
            (
                PathBuf::from("/bin/sh"),
                "-c 'while read -r line; do :; done'",
            )
        };
        std::fs::copy(fixture, &executable).unwrap();
        std::fs::write(bin.join("claude"), "#!/bin/sh\n[ \"$1\" = agents ] && [ \"$2\" = --json ] || exit 9\ncat \"$CLAUDE_CONFIG_DIR/active.json\"\n").unwrap();
        std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let id = Uuid::new_v4().to_string();
        // The filename intentionally differs from the native session UUID.
        std::fs::write(config.join("projects/demo/transcript.jsonl"), format!("{}\n{}\n",
            serde_json::json!({"type":"user", "uuid":"prompt-1", "parentUuid":null, "sessionId":id, "cwd":cwd, "timestamp":"2026-09-17T00:00:00Z", "message":{"role":"user","content":"Read the current conversation"}}),
            serde_json::json!({"type":"custom-title", "customTitle":"Native session name"})
        )).unwrap();
        let socket = root.join("terminald/terminald.sock");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket);
        let manager = TerminalManager::in_memory(client.clone());
        let tab = manager
            .create(CreateTerminalRequest {
                name: None,
                cwd: Some(root.to_string_lossy().into_owned()),
                workspace_root: None,
                shell: Some("/bin/sh".into()),
                agent_id: None,
                resume_session_id: None,
                rows: None,
                cols: None,
            })
            .await
            .unwrap();
        let pane_id = &tab.panes[0].id;
        let mut stream = client.attach(pane_id).await.unwrap();
        stream.send(tungstenite::Message::Binary(format!(
            "cd '{}'; HOME='{}' CLAUDE_CONFIG_DIR='{}' PATH='{}:/usr/bin:/bin' AOW_NATIVE_ENV_FIXTURE=1 '{}' {arguments}\n",
            cwd.display(), root.display(), config.display(), bin.display(), executable.display()
        ).into_bytes().into())).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let process = loop {
            if let Some(process) = manager.agents(None).await.unwrap().processes.get(pane_id) {
                break process.clone();
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        std::fs::write(
            config.join("active.json"),
            serde_json::json!([
                {"pid":process.pid, "id":"short-job", "sessionId":id, "cwd":cwd}
            ])
            .to_string(),
        )
        .unwrap();
        let app = crate::build_router(AppState {
            base_path: crate::BasePath::default(),
            frontend_dist: PathBuf::new(),
            auth: crate::auth::PinAuth::disabled(),
            session_shares: crate::session_shares::SessionShares::in_memory(),
            terminals: manager.clone(),
            aow: crate::aow::AowManager::in_memory(),
            automations: None,
            operations: crate::operations::OperationService::in_memory(),
            review_providers: crate::pull_requests::ProviderManager::default(),
        });
        let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/terminals/{}/panes/{pane_id}/agent-sessions",
                    tab.id
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["cwd"], cwd.to_string_lossy().as_ref());
        assert_eq!(body["live_session_id"], id);
        assert_eq!(body["sessions"][0]["title"], "Native session name");
        assert_eq!(body["sessions"][0]["session_id"], id);
        let response = app
            .oneshot(
                Request::get(format!(
                    "/api/aow/agent-sessions/{id}/snapshot?agent=claude&worktree_path={}",
                    cwd.display()
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["turns"][0]["user"]["text"],
            "Read the current conversation"
        );
        // Continue through the real notification loop using this same native
        // PID and custom config, not a manually registered transcript reader.
        let mut stops = manager.inner.task_stops.subscribe();
        manager.start_agent_notifications(crate::aow::AowManager::in_memory());
        let event = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                use std::io::Write;
                let mut transcript = std::fs::OpenOptions::new()
                    .append(true)
                    .open(config.join("projects/demo/transcript.jsonl"))
                    .unwrap();
                writeln!(
                    transcript,
                    "{}",
                    serde_json::json!({
                        "type":"system", "subtype":"turn_duration", "sessionId":id
                    })
                )
                .unwrap();
                match tokio::time::timeout(Duration::from_millis(200), stops.recv()).await {
                    Ok(event) => break event.unwrap(),
                    Err(_) => continue, // first registration starts at EOF
                }
            }
        })
        .await
        .expect("native process completion was not delivered");
        assert_eq!(event.agent, "claude");
        assert_eq!(event.session_id, id);
        assert_eq!(event.instance_ids, [pane_id.clone()]);
        assert_eq!(manager.get_snapshot(&tab.id).unwrap(), tab);
        drop(stream);
        stop_daemon(shutdown, daemon).await;
    }

    pub(super) async fn start_daemon(
        socket: PathBuf,
    ) -> (
        oneshot::Sender<()>,
        tokio::task::JoinHandle<Result<(), aow_terminald::TerminaldError>>,
    ) {
        let (shutdown, stopped) = oneshot::channel();
        let daemon_socket = socket.clone();
        let daemon = tokio::spawn(async move {
            aow_terminald::run_with_shutdown(daemon_socket, async move {
                let _ = stopped.await;
            })
            .await
        });
        let client = TerminaldClient::new(socket);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if client.health().await.is_ok() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "terminald did not become ready"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        (shutdown, daemon)
    }

    pub(super) async fn stop_daemon(
        shutdown: oneshot::Sender<()>,
        daemon: tokio::task::JoinHandle<Result<(), aow_terminald::TerminaldError>>,
    ) {
        let _ = shutdown.send(());
        tokio::time::timeout(std::time::Duration::from_secs(10), daemon)
            .await
            .expect("terminald shutdown timed out")
            .expect("terminald task panicked")
            .expect("terminald shutdown failed");
    }
}
