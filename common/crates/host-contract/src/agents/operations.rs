use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub agent_type: Option<AgentType>,
    pub display_name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverAgents {
    pub profiles: Vec<AgentProfile>,
    pub default_agent_id: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolveAgent {
    pub profile: AgentProfile,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareTerminal {
    pub launch: AgentLaunch,
    pub pane_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub resume_session_id: Option<String>,
    #[serde(default)]
    pub managed: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareRole {
    pub launch: AgentLaunch,
    pub instruction: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareRoleSession {
    pub launch: AgentLaunch,
    pub instruction: String,
    pub prompt: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct RoleSessionPlan {
    pub launch: AgentLaunch,
    pub initial_arguments: Vec<String>,
    pub task: Option<String>,
    pub managed: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateAutomation {
    pub agent: AgentType,
    pub arguments: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MatchTerminalProfile {
    pub profiles: Vec<AgentRegistration>,
    pub agent: String,
    pub executable: String,
    pub arguments: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum TerminalAgentOperation {
    Initialize {
        agent: String,
        task: Option<String>,
        timeout_seconds: u64,
    },
    Submit {
        task: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalAgentRequest {
    pub request_id: String,
    pub operation: TerminalAgentOperation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalAgentProgress {
    pub request_id: String,
    pub finished: bool,
    pub task_submitted: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeAgent {
    pub launch: AgentLaunch,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolveRoleLayout {
    pub agent_id: String,
}
