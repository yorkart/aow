use std::path::{Path, PathBuf};

use aow_terminald_client::TerminaldClient;

use crate::{
    BasePath, TerminalError, aow, auth, automations, git_platform_providers, operations,
    session_shares, terminal, workspace_events,
};

#[derive(Clone)]
pub struct AppState {
    pub(crate) file_search: crate::filesystem::search::SearchRequests,
    pub(crate) acp: aow_acp::AcpService,
    pub(crate) acp_controls: crate::acp::Controls,
    pub(crate) instances: crate::instances::InstanceStore,
    pub(crate) inbox: crate::inbox::InboxStore,
    pub(crate) base_path: BasePath,
    pub(crate) frontend_dist: PathBuf,
    pub(crate) auth: auth::AuthService,
    pub(crate) session_shares: session_shares::SessionShares,
    pub(crate) terminals: terminal::TerminalManager,
    pub(crate) aow: aow::AowManager,
    pub(crate) automations: Option<automations::AutomationManager>,
    pub(crate) operations: operations::OperationService,
    pub(crate) workspace_events: workspace_events::WorkspaceEvents,
    pub(crate) git_platform_providers: git_platform_providers::GitPlatformProviderManager,
}

impl AppState {
    /// Creates an application state with in-memory terminal metadata. This is
    /// useful for embedding and tests; the server binary uses `with_state_dir`.
    pub fn new(frontend_dist: PathBuf) -> Self {
        Self::with_terminald_socket(frontend_dist, TerminaldClient::default_socket_path())
    }

    pub fn with_terminald_socket(frontend_dist: PathBuf, terminald_socket: PathBuf) -> Self {
        let aow = aow::AowManager::in_memory();
        let terminals =
            terminal::TerminalManager::in_memory(TerminaldClient::new(terminald_socket));
        let workspace_events = workspace_events::WorkspaceEvents::new();
        Self {
            file_search: crate::filesystem::search::SearchRequests::default(),
            acp: crate::acp::service(
                aow.clone(),
                None,
                terminals.task_stop_sender(),
                workspace_events.clone(),
            )
            .expect("in-memory ACP service"),
            acp_controls: crate::acp::Controls::default(),
            instances: crate::instances::InstanceStore::default(),
            inbox: crate::inbox::InboxStore::in_memory(),
            base_path: BasePath::default(),
            frontend_dist,
            auth: auth::AuthService::disabled(),
            session_shares: session_shares::SessionShares::in_memory(),
            terminals,
            aow,
            automations: None,
            git_platform_providers: git_platform_providers::GitPlatformProviderManager::default(),
            operations: operations::OperationService::in_memory(),
            workspace_events,
        }
    }

    pub fn with_state_dir(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        terminald_socket: PathBuf,
    ) -> Result<Self, TerminalError> {
        Self::with_runtime_options(frontend_dist, state_dir, terminald_socket)
    }

    pub fn with_runtime_options(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        terminald_socket: PathBuf,
    ) -> Result<Self, TerminalError> {
        let aow = aow::AowManager::persistent(&state_dir).map_err(|error| {
            TerminalError::Invalid(format!("failed to initialize aow: {error}"))
        })?;
        let terminals = terminal::TerminalManager::persistent(
            state_dir.clone(),
            TerminaldClient::new(terminald_socket),
        )?;
        let workspace_events = workspace_events::WorkspaceEvents::new();
        Ok(Self {
            file_search: crate::filesystem::search::SearchRequests::default(),
            acp_controls: crate::acp::Controls::default(),
            acp: crate::acp::service(
                aow.clone(),
                Some(state_dir.clone()),
                terminals.task_stop_sender(),
                workspace_events.clone(),
            )
            .map_err(|error| {
                TerminalError::Invalid(format!("failed to initialize ACP: {error:#}"))
            })?,
            instances: crate::instances::InstanceStore::persistent(&state_dir).map_err(|e| {
                TerminalError::Invalid(format!("failed to initialize instance history: {e}"))
            })?,
            inbox: crate::inbox::InboxStore::persistent(&state_dir)
                .map_err(|e| TerminalError::Invalid(format!("failed to initialize inbox: {e}")))?,
            workspace_events,
            git_platform_providers: git_platform_providers::GitPlatformProviderManager::persistent(
                &state_dir,
            )
            .map_err(|e| TerminalError::Invalid(e.to_string()))?,
            base_path: BasePath::default(),
            frontend_dist,
            operations: operations::OperationService::persistent(
                &state_dir.join("state/operation-logs"),
            )
            .map_err(|error| TerminalError::Invalid(error.to_string()))?,
            auth: auth::AuthService::persistent(&state_dir),
            session_shares: session_shares::SessionShares::persistent(&state_dir).map_err(
                |error| {
                    TerminalError::Invalid(format!("failed to initialize session shares: {error}"))
                },
            )?,
            automations: Some(
                automations::AutomationManager::new(state_dir.clone(), aow.notifications().clone())
                    .map_err(|error| {
                        TerminalError::Invalid(format!("failed to initialize automations: {error}"))
                    })?,
            ),
            terminals,
            aow,
        })
    }

    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    pub fn with_secure_cookies(mut self, enabled: bool) -> Self {
        self.auth.secure_cookies = enabled;
        self
    }

    /// Sets login session limits before serving requests or cloning the state.
    pub fn with_session_timeouts(mut self, timeouts: crate::SessionTimeouts) -> Self {
        self.auth.set_session_timeouts(timeouts);
        self
    }

    /// Reconciles durable terminal tabs with the external terminal daemon.
    /// A later terminal request retries this operation if startup happens
    /// while the daemon is unavailable.
    pub async fn reconcile_terminals(&self) -> Result<(), TerminalError> {
        self.terminals.reconcile().await
    }

    /// The ACP writer runs outside Tokio; explicitly drain it before the runtime exits.
    pub async fn flush_acp_history(&self) -> anyhow::Result<()> {
        let service = self.acp.clone();
        aow_i18n::spawn_blocking(move || service.flush_threads_on_quit()).await?
    }

    pub fn start_agent_notifications(&self) {
        self.terminals.start_agent_notifications(self.aow.clone());
        terminal::hosting::recover(self.clone());
    }

    pub async fn initialize_global_workspace(&self) -> anyhow::Result<()> {
        self.aow.initialize_global().await?;
        Ok(())
    }
}

pub async fn initialize_aow_state(path: &Path) -> anyhow::Result<()> {
    git_platform_providers::GitPlatformProviderManager::persistent(path)?;
    aow::AowManager::persistent(path)?
        .initialize_global()
        .await?;
    Ok(())
}
