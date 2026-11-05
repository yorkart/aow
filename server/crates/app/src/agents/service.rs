use super::*;
use crate::runtime_connection::RuntimeConnection;
#[derive(Clone)]
pub(crate) struct AgentService {
    pub(super) inner: Arc<AgentState>,
    pub(super) node: RuntimeConnection,
    pub(super) cli_access: aow_cli_access::Access,
}
pub(super) struct AgentState {
    pub(super) config: Option<aow_config::ConfigStore>,
    pub(super) agents_path: Option<PathBuf>,
    pub(super) state: Mutex<RegistryState>,
    pub(super) agent_operation: tokio::sync::Mutex<()>,
    pub(super) session_locators:
        Mutex<std::collections::HashMap<String, sessions::CachedSessionLocator>>,
}
impl AgentService {
    pub(crate) fn new(
        config: Option<aow_config::ConfigStore>,
        node: RuntimeConnection,
        cli_access: aow_cli_access::Access,
    ) -> Result<Self, ServiceError> {
        let agents_path = config
            .as_ref()
            .map(|c| c.directory().join("aow-agents.json"));
        let (agents, default_agent_id) = agents_path
            .as_ref()
            .map(|p| storage::load_agents(p))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            node,
            cli_access,
            inner: Arc::new(AgentState {
                config,
                agents_path,
                state: Mutex::new(RegistryState {
                    agents,
                    default_agent_id,
                }),
                agent_operation: Default::default(),
                session_locators: Default::default(),
            }),
        })
    }
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, RegistryState>, ServiceError> {
        self.inner.state.lock().map_err(|_| ServiceError::Poisoned)
    }
    pub(crate) fn callback_environment(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, ServiceError> {
        self.cli_access
            .environment()
            .map_err(|e| ServiceError::Invalid(e.to_string()))
    }
}

#[cfg(test)]
impl AgentService {
    pub(crate) fn test_registry(&self) -> Result<MutexGuard<'_, RegistryState>, ServiceError> {
        self.lock()
    }
    pub(crate) fn test_registry_path(&self) -> Option<&Path> {
        self.inner.agents_path.as_deref()
    }
}
