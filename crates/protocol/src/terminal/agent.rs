use std::collections::BTreeMap;

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
pub struct AgentTerminalCreate {
    pub agent: String,
    pub project_id: String,
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
    pub tab_id: String,
    pub cwd: String,
    pub agent: String,
    pub status: TerminalPaneStatus,
    #[serde(flatten)]
    pub state: AgentTerminalState,
}

/// Live agent identities by runtime ID; null means no agent was detected.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAgentList {
    pub agents: BTreeMap<String, Option<String>>,
    /// Last nonempty OSC 0/2 title for each running runtime, independent of attachments.
    /// Missing entries mean no title is available; older daemons omit this field.
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    /// Foreground agent processes only. Missing on older daemons and platforms
    /// without process inspection; title-based selection remains available.
    #[serde(default)]
    pub processes: BTreeMap<String, TerminalAgentProcess>,
    /// Live shell state used to label panes when no OSC title is available.
    /// Missing on older daemons and platforms without process inspection.
    #[serde(default)]
    pub activity: BTreeMap<String, TerminalPaneActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalPaneActivity {
    /// Current working directory of the shell process.
    pub cwd: Option<String>,
    /// Executable name of the foreground process, absent at the shell prompt.
    pub foreground_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAgentProcess {
    pub pid: i32,
    /// Opaque OS process start identity distinguishes PID reuse. Linux uses
    /// clock ticks; macOS uses seconds and microseconds. Compare as a string.
    pub start_time: String,
    pub cwd: String,
    /// AoW's per-launch Pi identity file. Pi can rewrite its native environment
    /// when setting the process title, so terminald retains this one safe path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_binding: Option<String>,
}
