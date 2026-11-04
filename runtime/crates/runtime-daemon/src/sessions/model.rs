use aow_agents::sessions::AgentSessionLocator;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) use aow_runtime_contract::NativeSessionReference as SessionReference;
#[derive(Serialize, Deserialize)]
pub(super) struct SavedLocator {
    pub agent: String,
    pub session_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub transcript_path: PathBuf,
    pub trusted_root: PathBuf,
}
impl From<&AgentSessionLocator> for SavedLocator {
    fn from(value: &AgentSessionLocator) -> Self {
        Self {
            agent: value.agent.into(),
            session_id: value.session_id.clone(),
            title: value.title.clone(),
            cwd: value.cwd.clone(),
            transcript_path: value.transcript_path.clone(),
            trusted_root: value.trusted_root.clone(),
        }
    }
}
impl SavedLocator {
    pub fn into_locator(self) -> anyhow::Result<AgentSessionLocator> {
        let agent = aow_agents::Agent::from_id(&self.agent)
            .ok_or_else(|| anyhow::anyhow!("Unsupported native session provider"))?;
        Ok(AgentSessionLocator {
            agent: agent.id(),
            session_id: self.session_id,
            title: self.title,
            cwd: self.cwd,
            transcript_path: self.transcript_path,
            trusted_root: self.trusted_root,
        })
    }
}
