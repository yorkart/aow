use crate::project::agent_registry_store::{AgentRegistryStore, current_platform_key};
use acp_client::{
    AcpHost, AgentInfo, OperationProgress as Progress, SyncEvents, validate_settings,
};
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
};

#[derive(Clone)]
pub(crate) struct AdapterManager {
    pub(crate) inner: Arc<State>,
}
pub(crate) struct State {
    pub host: Arc<dyn AcpHost>,
    pub cache: PathBuf,
    pub registry: AgentRegistryStore,
    pub connect_lock: tokio::sync::Mutex<()>,
    pub shutting_down: AtomicBool,
    pub installations: Mutex<HashMap<String, Progress>>,
    pub events: OnceLock<SyncEvents>,
}
impl AdapterManager {
    pub fn new(host: Arc<dyn AcpHost>, cache: Option<PathBuf>) -> Self {
        let registry = cache.as_ref().map(|path| path.join("registry.json"));
        Self {
            inner: Arc::new(State {
                host,
                cache: cache.unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("aow-acp-{}", std::process::id()))
                }),
                registry: AgentRegistryStore::new(registry),
                connect_lock: Default::default(),
                shutting_down: AtomicBool::new(false),
                installations: Default::default(),
                events: Default::default(),
            }),
        }
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        let raw = self.inner.host.settings().await?;
        let settings = validate_settings(&raw)?;
        let values: Value = serde_json_lenient::from_str(&raw)?;
        let execution_path = self.inner.host.execution_path().await?;
        Ok(settings
            .agent_servers
            .keys()
            .map(|id| AgentInfo {
                id: id.clone(),
                name: id.clone(),
                description: String::new(),
                version: None,
                icon: None,
                repository: None,
                website: None,
                license_url: None,
                configured: true,
                supported: true,
                installed: self.installed(id, &values, execution_path.as_deref()),
            })
            .collect())
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        let configured = self.agents().await?;
        Ok(self
            .inner
            .registry
            .agents(refresh)
            .await?
            .into_iter()
            .map(|entry| {
                let supported = entry.distribution.npx.is_some()
                    || current_platform_key().is_some_and(|key| {
                        entry
                            .distribution
                            .binary
                            .as_ref()
                            .is_some_and(|targets| targets.contains_key(key))
                    });
                AgentInfo {
                    configured: configured.iter().any(|agent| agent.id == entry.id),
                    installed: configured
                        .iter()
                        .any(|agent| agent.id == entry.id && agent.installed),
                    id: entry.id,
                    name: entry.name,
                    description: entry.description,
                    version: Some(entry.version),
                    icon: entry.icon,
                    repository: entry.repository,
                    website: entry.website,
                    license_url: entry.license_url,
                    supported,
                }
            })
            .collect())
    }
}
