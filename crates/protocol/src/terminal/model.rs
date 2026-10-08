use serde::{Deserialize, Serialize};

use super::agent::AgentTerminalState;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPaneStatus {
    Running,
    Exited,
    Interrupted,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPaneKind {
    #[default]
    Terminal,
    Agent,
}

impl TerminalPaneKind {
    fn is_terminal(&self) -> bool {
        *self == Self::Terminal
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSplitAxis {
    /// Places the children left-to-right.
    Row,
    /// Places the children top-to-bottom.
    Column,
}

fn default_terminal_split_ratio() -> f32 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalLayout {
    Pane {
        pane_id: String,
    },
    Split {
        axis: TerminalSplitAxis,
        #[serde(default = "default_terminal_split_ratio")]
        ratio: f32,
        first: Box<TerminalLayout>,
        second: Box<TerminalLayout>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalPane {
    pub id: String,
    /// Pane that created this agent. Retained even after the parent is removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pane_id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "TerminalPaneKind::is_terminal")]
    pub kind: TerminalPaneKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Registration used to launch the agent; distinct from its product type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_id: Option<String>,
    /// Present only for CLI-created, initially hidden interactive agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_terminal: Option<AgentTerminalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosting: Option<super::TerminalHosting>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub restart_on_daemon_restart: bool,
    pub status: TerminalPaneStatus,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalTab {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_is_custom: Option<bool>,
    pub workspace_root: String,
    pub layout: TerminalLayout,
    pub panes: Vec<TerminalPane>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalTabList {
    pub tabs: Vec<TerminalTab>,
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}
