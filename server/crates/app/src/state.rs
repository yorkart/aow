use std::path::{Path, PathBuf};

use aow_host_client::{HostClient, HostEndpoint, TerminalClient};

use crate::{
    BasePath, TerminalError, auth, automations, git_platform_providers, operations, session_shares,
    terminal, workspace_events,
};

#[derive(Clone)]
pub struct AppState {
    pub(crate) collaboration: crate::collaboration::Runtime,
    pub(crate) file_search: crate::filesystem::search::SearchRequests,
    pub(crate) acp: aow_host_client::acp::AcpClient,
    pub(crate) acp_controls: crate::acp::Controls,
    pub(crate) instances: crate::instances::InstanceStore,
    pub(crate) base_path: BasePath,
    pub(crate) frontend_dist: PathBuf,
    pub(crate) auth: auth::AuthService,
    pub(crate) session_shares: session_shares::SessionShares,
    pub(crate) terminals: terminal::TerminalManager,
    pub(crate) projects: crate::projects::ProjectService,
    pub(crate) agents: crate::agents::AgentService,
    pub(crate) roles: crate::roles::RoleService,
    pub(crate) tasks: crate::tasks::TaskService,
    pub(crate) settings: crate::settings::SettingsService,
    pub(crate) node: crate::host_connection::HostConnection,
    pub(crate) notifications: crate::notifications::NotificationManager,
    pub(crate) cli_access: aow_cli_access::Access,
    pub(crate) automations: Option<automations::AutomationManager>,
    pub(crate) operations: operations::OperationService,
    pub(crate) workspace_events: workspace_events::WorkspaceEvents,
    pub(crate) git_platform_providers: git_platform_providers::GitPlatformProviderManager,
}

impl AppState {
    /// Creates an application state with in-memory terminal metadata. This is
    /// useful for embedding and tests; the server binary uses `with_host`.
    pub fn for_test(frontend_dist: PathBuf) -> Self {
        Self::for_test_with_host_config(frontend_dist, PathBuf::new())
    }

    pub fn for_test_with_host_config(frontend_dist: PathBuf, host_config: PathBuf) -> Self {
        let (projects, agents, roles, tasks, settings, node, cli_access) =
            (|| -> Result<_, crate::error::ServiceError> {
                let runtime = None;
                let directory = None;
                let config = None;
                let notes_base = crate::settings::default_notes_base();
                let cli_access = aow_cli_access::Access::default();
                let node = crate::host_connection::HostConnection::new(
                    runtime.clone(),
                    directory.as_deref(),
                    config.clone(),
                );
                let settings = crate::settings::SettingsService::new(
                    config.clone(),
                    notes_base,
                    node.clone(),
                )?;
                let projects = crate::projects::ProjectService::new(
                    directory.clone(),
                    config.clone(),
                    settings.clone(),
                    node.clone(),
                )?;
                let agents = crate::agents::AgentService::new(
                    config.clone(),
                    node.clone(),
                    cli_access.clone(),
                )?;
                let roles = crate::roles::RoleService::new(
                    directory.clone(),
                    config,
                    node.clone(),
                    agents.clone(),
                    projects.clone(),
                )?;
                let tasks = crate::tasks::TaskService::new(directory, roles.clone(), node.clone())?;
                Ok((projects, agents, roles, tasks, settings, node, cli_access))
            })()
            .expect("in-memory business services");
        let notifications = crate::notifications::NotificationManager::in_memory();
        let terminals =
            terminal::TerminalManager::in_memory(TerminalClient::for_test_config(host_config));
        let workspace_events = workspace_events::WorkspaceEvents::new();
        Self {
            collaboration: Default::default(),
            file_search: crate::filesystem::search::SearchRequests::default(),
            acp: crate::acp::service(
                &projects,
                &notifications,
                &cli_access,
                &node,
                None,
                terminals.task_stop_sender(),
                workspace_events.clone(),
            )
            .expect("in-memory ACP service"),
            acp_controls: crate::acp::Controls::default(),
            instances: crate::instances::InstanceStore::default(),
            base_path: BasePath::default(),
            frontend_dist,
            auth: auth::AuthService::disabled(),
            session_shares: session_shares::SessionShares::in_memory(),
            terminals,
            projects,
            agents,
            roles,
            tasks,
            settings,
            node,
            notifications,
            cli_access,
            automations: None,
            git_platform_providers: git_platform_providers::GitPlatformProviderManager::default(),
            operations: operations::OperationService::in_memory(),
            workspace_events,
        }
    }

    pub fn for_test_with_state_dir(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        config: PathBuf,
    ) -> Result<Self, TerminalError> {
        Self::build(
            frontend_dist,
            state_dir,
            TerminalClient::for_test_config(config),
            None,
        )
    }
    pub fn with_host(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        endpoint: HostEndpoint,
    ) -> Result<Self, TerminalError> {
        let runtime = HostClient::new(endpoint);
        Self::build(frontend_dist, state_dir, runtime.terminal(), Some(runtime))
    }
    #[cfg(test)]
    pub(crate) fn with_test_host_client(
        state_dir: PathBuf,
        runtime: HostClient,
    ) -> Result<Self, TerminalError> {
        Self::build(PathBuf::new(), state_dir, runtime.terminal(), Some(runtime))
    }

    fn build(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        terminal: TerminalClient,
        runtime: Option<HostClient>,
    ) -> Result<Self, TerminalError> {
        let config = Some(
            aow_config::ConfigStore::initialize(&state_dir)
                .map_err(|error| TerminalError::Invalid(error.to_string()))?,
        );
        let directory = Some(std::fs::canonicalize(&state_dir)?);
        let notes_base = directory.as_ref().unwrap().join("data/notes");
        let (projects, agents, roles, tasks, settings, node, cli_access) =
            (|| -> Result<_, crate::error::ServiceError> {
                let cli_access = aow_cli_access::Access::default();
                let node = crate::host_connection::HostConnection::new(
                    runtime.clone(),
                    directory.as_deref(),
                    config.clone(),
                );
                let settings = crate::settings::SettingsService::new(
                    config.clone(),
                    notes_base,
                    node.clone(),
                )?;
                let projects = crate::projects::ProjectService::new(
                    directory.clone(),
                    config.clone(),
                    settings.clone(),
                    node.clone(),
                )?;
                let agents = crate::agents::AgentService::new(
                    config.clone(),
                    node.clone(),
                    cli_access.clone(),
                )?;
                let roles = crate::roles::RoleService::new(
                    directory.clone(),
                    config,
                    node.clone(),
                    agents.clone(),
                    projects.clone(),
                )?;
                let tasks = crate::tasks::TaskService::new(directory, roles.clone(), node.clone())?;
                Ok((projects, agents, roles, tasks, settings, node, cli_access))
            })()
            .map_err(|error| {
                TerminalError::Invalid(format!("failed to initialize business services: {error}"))
            })?;
        let notifications = crate::notifications::NotificationManager::persistent(&state_dir)
            .map_err(|error| TerminalError::Invalid(error.to_string()))?;
        #[cfg(test)]
        let execution_runtime = runtime.clone().or_else(|| {
            terminal
                .host()
                .is_configured()
                .then(|| terminal.host().clone())
        });
        #[cfg(not(test))]
        let execution_runtime = runtime.clone();
        let terminals = terminal::TerminalManager::persistent(state_dir.clone(), terminal)?;
        let workspace_events = workspace_events::WorkspaceEvents::new();
        Ok(Self {
            collaboration: Default::default(),
            file_search: crate::filesystem::search::SearchRequests::default(),
            acp_controls: crate::acp::Controls::default(),
            acp: crate::acp::service(
                &projects,
                &notifications,
                &cli_access,
                &node,
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
            workspace_events,
            git_platform_providers:
                git_platform_providers::GitPlatformProviderManager::persistent_with_host(
                    &state_dir,
                    runtime.clone(),
                )
                .map_err(|e| TerminalError::Invalid(e.to_string()))?,
            base_path: BasePath::default(),
            frontend_dist,
            operations: operations::OperationService::persistent(
                &state_dir.join("state/operation-logs"),
            )
            .map_err(|error| TerminalError::Invalid(error.to_string()))?,
            auth: auth::AuthService::persistent(&state_dir),
            session_shares: session_shares::SessionShares::persistent(&state_dir)
                .map(|shares| shares.with_host(runtime.clone()))
                .map_err(|error| {
                    TerminalError::Invalid(format!("failed to initialize session shares: {error}"))
                })?,
            automations: Some(
                automations::AutomationManager::new(state_dir.clone(), notifications.clone())
                    .and_then(|manager| manager.with_host(execution_runtime.as_ref()))
                    .map_err(|error| {
                        TerminalError::Invalid(format!("failed to initialize automations: {error}"))
                    })?,
            ),
            terminals,
            projects,
            agents,
            roles,
            tasks,
            settings,
            node,
            notifications,
            cli_access,
        })
    }

    pub fn with_cli_callback(self, url: &str, token_file: Option<PathBuf>) -> anyhow::Result<Self> {
        aow_server_client::ServerClient::remote(url, "validation-token".into())?;
        let state_dir = self
            .automations
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CLI callbacks require persistent state"))?
            .store()
            .state_dir
            .clone();
        self.cli_access.configure(&state_dir, url, token_file)?;
        Ok(self)
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
        service.flush_threads_on_quit().await
    }

    pub async fn acp_recover_runtime(&self) -> anyhow::Result<()> {
        self.acp.recover_workspace_removals().await
    }

    pub fn start_agent_notifications(&self) {
        self.terminals
            .start_agent_notifications(self.projects.clone(), self.notifications.clone());
        terminal::hosting::recover(self.clone());
    }

    pub async fn initialize_global_workspace(&self) -> anyhow::Result<()> {
        self.projects.initialize_global().await?;
        Ok(())
    }
}

pub async fn initialize_aow_state(path: &Path) -> anyhow::Result<()> {
    let config = aow_config::ConfigStore::initialize(path)?;
    let directory = std::fs::canonicalize(path)?;
    let node =
        crate::host_connection::HostConnection::new(None, Some(&directory), Some(config.clone()));
    let settings = crate::settings::SettingsService::new(
        Some(config.clone()),
        directory.join("data/notes"),
        node.clone(),
    )?;
    let projects = crate::projects::ProjectService::new(
        Some(directory.clone()),
        Some(config.clone()),
        settings,
        node.clone(),
    )?;
    let agents =
        crate::agents::AgentService::new(Some(config.clone()), node.clone(), Default::default())?;
    let roles = crate::roles::RoleService::new(
        Some(directory.clone()),
        Some(config),
        node.clone(),
        agents,
        projects,
    )?;
    let _tasks = crate::tasks::TaskService::new(Some(directory.clone()), roles, node)?;
    let _notifications = crate::notifications::NotificationManager::persistent(&directory)?;
    git_platform_providers::seed_configuration(path)?;
    // Node workspaces are initialized after Server connects to its Host endpoint.
    Ok(())
}
