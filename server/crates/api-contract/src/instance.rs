use aow_runtime_contract::instance::InstanceRef;
use serde::{Deserialize, Serialize};

/// Execution context of the caller, independent of the newly created instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationContext {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_connection_id: Option<String>,
}

impl CreationContext {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.trim().is_empty()
            || self.session_id.len() > 1024
            || self.session_id.chars().any(char::is_control)
        {
            return Err(
                "caller session ID must contain 1-1024 bytes without control characters".into(),
            );
        }
        if let Some(id) = &self.acp_connection_id {
            InstanceRef::acp(id).validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<CreationContext>,
    #[serde(default)]
    pub no_parent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub project_id: String,
    pub cwd: String,
    #[serde(default)]
    pub parent: Option<InstanceRef>,
    pub task: Option<String>,
    pub timeout_seconds: u64,
}

/// Creation metadata retained after the live instance has been deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    #[serde(flatten)]
    pub instance: InstanceRef,
    pub parent: Option<InstanceRef>,
    pub title: String,
    pub cwd: String,
    pub agent_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}
