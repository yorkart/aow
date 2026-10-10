//! Source: Zed project/src/agent_registry_store.rs. Runtime and storage are host adaptations.
use crate::acp_thread::history::atomic_write;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

pub(crate) const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RegistryEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub distribution: RegistryDistribution,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RegistryDistribution {
    pub binary: Option<BTreeMap<String, RegistryTargetConfig>>,
    pub npx: Option<RegistryNpxDistribution>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RegistryTargetConfig {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub sha256: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RegistryNpxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct RegistryIndex {
    agents: Vec<RegistryEntry>,
}

pub(crate) struct AgentRegistryStore {
    cache: Option<PathBuf>,
    client: reqwest::Client,
}
impl AgentRegistryStore {
    pub(crate) fn new(cache: Option<PathBuf>) -> Self {
        Self {
            cache,
            client: reqwest::Client::new(),
        }
    }
    pub(crate) async fn agents(&self, refresh: bool) -> Result<Vec<RegistryEntry>> {
        if !refresh
            && let Some(path) = &self.cache
            && let Ok(bytes) = tokio::fs::read(path).await
        {
            return Ok(serde_json::from_slice::<RegistryIndex>(&bytes)?.agents);
        }
        let response = self
            .client
            .get(REGISTRY_URL)
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        let index: RegistryIndex =
            serde_json::from_slice(&bytes).context("Invalid ACP Registry index")?;
        if let Some(path) = &self.cache {
            atomic_write(path, &bytes)?;
        }
        Ok(index.agents)
    }
}
pub(crate) fn current_platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-aarch64"),
        ("macos", "x86_64") => Some("darwin-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        _ => None,
    }
}
