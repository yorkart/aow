use std::path::{Path, PathBuf};

use aow_terminald_client::TerminaldClient;

use crate::{
    BasePath, TerminalError, aow, auth, automations, operations, pull_requests, session_shares,
    terminal, workspace_events,
};

#[derive(Clone)]
pub struct AppState {
    pub(crate) base_path: BasePath,
    pub(crate) frontend_dist: PathBuf,
    pub(crate) auth: auth::AuthService,
    pub(crate) session_shares: session_shares::SessionShares,
    pub(crate) terminals: terminal::TerminalManager,
    pub(crate) aow: aow::AowManager,
    pub(crate) automations: Option<automations::AutomationManager>,
    pub(crate) operations: operations::OperationService,
    pub(crate) workspace_events: workspace_events::WorkspaceEvents,
    pub(crate) review_providers: pull_requests::ProviderManager,
}

impl AppState {
    /// Creates an application state with in-memory terminal metadata. This is
    /// useful for embedding and tests; the server binary uses `with_state_dir`.
    pub fn new(frontend_dist: PathBuf) -> Self {
        Self::with_terminald_socket(frontend_dist, TerminaldClient::default_socket_path())
    }

    pub fn with_terminald_socket(frontend_dist: PathBuf, terminald_socket: PathBuf) -> Self {
        Self {
            base_path: BasePath::default(),
            frontend_dist,
            auth: auth::AuthService::disabled(),
            session_shares: session_shares::SessionShares::in_memory(),
            terminals: terminal::TerminalManager::in_memory(TerminaldClient::new(terminald_socket)),
            aow: aow::AowManager::in_memory(),
            automations: None,
            review_providers: pull_requests::ProviderManager::default(),
            operations: operations::OperationService::in_memory(),
            workspace_events: workspace_events::WorkspaceEvents::new(),
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
        Ok(Self {
            workspace_events: workspace_events::WorkspaceEvents::new(),
            review_providers: pull_requests::ProviderManager::persistent(&state_dir)
                .map_err(|e| TerminalError::Invalid(e.to_string()))?,
            base_path: BasePath::default(),
            frontend_dist,
            operations: operations::OperationService::persistent(&state_dir.join("operation-logs"))
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
            terminals: terminal::TerminalManager::persistent(
                state_dir.clone(),
                TerminaldClient::new(terminald_socket),
            )?,
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

    /// Reconciles durable terminal tabs with the external terminal daemon.
    /// A later terminal request retries this operation if startup happens
    /// while the daemon is unavailable.
    pub async fn reconcile_terminals(&self) -> Result<(), TerminalError> {
        self.terminals.reconcile().await
    }

    pub fn start_agent_notifications(&self) {
        self.terminals.start_agent_notifications(self.aow.clone());
    }

    pub async fn initialize_global_workspace(&self) -> anyhow::Result<()> {
        self.aow.initialize_global().await?;
        Ok(())
    }
}

pub async fn initialize_aow_state(path: &Path) -> anyhow::Result<()> {
    pull_requests::ProviderManager::persistent(path)?;
    aow::AowManager::persistent(path)?
        .initialize_global()
        .await?;
    Ok(())
}
