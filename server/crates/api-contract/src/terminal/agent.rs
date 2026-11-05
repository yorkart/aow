use serde::{Deserialize, Serialize};

use super::model::TerminalPaneStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTerminalPhase {
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTerminalState {
    pub phase: AgentTerminalPhase,
    pub error: Option<String>,
    pub task_submitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<crate::instance::CreationContext>,
    #[serde(default)]
    pub no_parent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<aow_runtime_contract::instance::InstanceRef>,
    pub cwd: String,
    pub task: Option<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTerminalSubmit {
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTerminalInfo {
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<aow_runtime_contract::instance::InstanceRef>,
    pub tab_id: String,
    pub cwd: String,
    pub agent: String,
    pub status: TerminalPaneStatus,
    #[serde(flatten)]
    pub state: AgentTerminalState,
}
