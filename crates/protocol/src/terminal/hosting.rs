use serde::{Deserialize, Serialize};

use super::TerminalAgentProcess;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalHostingPhase {
    Waiting,
    Reviewing,
    Collecting,
    Submitting,
    Completed,
    LimitReached,
    Failed,
}

impl TerminalHostingPhase {
    pub fn stopped(self) -> bool {
        matches!(self, Self::Completed | Self::LimitReached | Self::Failed)
    }
}

/// Durable ownership and progress of an automation attached to one live pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalHosting {
    pub id: String,
    pub task_id: String,
    pub task_revision: u64,
    pub task_name: String,
    pub workspace_root: String,
    pub agent: String,
    pub session_id: String,
    pub process: TerminalAgentProcess,
    pub phase: TerminalHostingPhase,
    pub phase_started_at: String,
    #[serde(default = "TerminalHosting::default_max_inputs")]
    pub max_inputs: u32,
    #[serde(default)]
    pub input_count: u32,
    pub source_turn_id: Option<String>,
    pub run_id: Option<String>,
    pub error: Option<String>,
}

impl TerminalHosting {
    pub const fn default_max_inputs() -> u32 {
        3
    }
}
