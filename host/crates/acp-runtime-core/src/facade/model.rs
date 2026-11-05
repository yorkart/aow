pub use aow_host_contract::acp::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    #[serde(flatten)]
    pub data: aow_host_contract::acp::SessionSnapshot,
    #[serde(skip)]
    pub(crate) session_meta_suppressed: bool,
    #[serde(skip)]
    pub(crate) last_prompt_usage: Option<Value>,
    #[serde(skip)]
    pub(crate) pending_user_echo: String,
    #[serde(skip)]
    pub(crate) steering: crate::acp_thread::steering::SteeringState,
    #[serde(skip)]
    pub(crate) pending_user_images: Vec<Value>,
    #[serde(skip)]
    pub(crate) active_prompt: Option<String>,
    #[serde(skip)]
    pub(crate) changes: crate::acp_thread::updates::ChangeLog,
    #[serde(skip)]
    pub(crate) overview_cache: crate::acp_thread::overview::OverviewCache,
}
impl std::ops::Deref for SessionSnapshot {
    type Target = aow_host_contract::acp::SessionSnapshot;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::DerefMut for SessionSnapshot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
impl From<&SessionSnapshot> for SessionInfo {
    fn from(value: &SessionSnapshot) -> Self {
        Self::from(&value.data)
    }
}
pub type SessionUpdates = aow_host_contract::acp::SessionUpdates<SessionSnapshot>;
