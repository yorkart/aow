use super::*;
use crate::settings::persistence::REGISTRY_VERSION;
use std::collections::BTreeMap;
#[derive(Default)]
pub(crate) struct RegistryState {
    pub(crate) agents: Vec<StoredAgent>,
    pub(crate) default_agent_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredAgent {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) agent_type: Option<AgentType>,
    pub(crate) display_name: String,
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentRegistryDocument {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_agent_id: Option<String>,
    items: Vec<StoredAgent>,
}

pub(crate) fn load_agents(path: &Path) -> Result<(Vec<StoredAgent>, Option<String>), ServiceError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: AgentRegistryDocument = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(ServiceError::Invalid(format!(
                    "unsupported AoW registry version {}",
                    document.version
                )));
            }
            Ok((document.items, document.default_agent_id))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((Vec::new(), None)),
        Err(error) => Err(error.into()),
    }
}

impl AgentService {
    pub(crate) fn persist_agents(
        &self,
        agents: &[StoredAgent],
        default_agent_id: Option<&str>,
    ) -> Result<(), ServiceError> {
        if let Some(path) = &self.inner.agents_path {
            crate::settings::persistence::save_configuration(
                self.inner.config.as_ref(),
                path,
                &AgentRegistryDocument {
                    version: REGISTRY_VERSION,
                    default_agent_id: default_agent_id.map(str::to_owned),
                    items: agents.to_vec(),
                },
            )?;
        }
        Ok(())
    }
}
