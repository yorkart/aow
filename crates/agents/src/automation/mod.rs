//! Native automation capability and built-in adapter dispatch.

mod claude;
mod codex;
mod codex_like;
mod traecli;

use crate::Agent;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Native execution without a terminal, always starting a new persistent session.
pub trait AgentAutomation {
    fn session_id_mode(&self) -> SessionIdMode;
    fn validate_arguments(&self, arguments: &[String]) -> Result<()>;
    fn automation_arguments(&self, yolo: bool, session_id: Option<&str>) -> Result<Vec<String>>;
    fn session_from_line(&self, line: &[u8]) -> Option<String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionIdMode {
    GeneratedUuid,
    FromOutput,
}

/// Only adapters with automation support can be stored in an automation task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAgent {
    Codex,
    #[serde(rename = "traecli")]
    TraeCli,
    Claude,
}

impl AutomationAgent {
    pub fn agent(self) -> Agent {
        match self {
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
            Self::Claude => Agent::Claude,
        }
    }
    pub fn id(self) -> &'static str {
        self.agent().id()
    }
}

impl Agent {
    pub fn automation(self) -> Option<AutomationAgent> {
        match self {
            Self::Codex => Some(AutomationAgent::Codex),
            Self::TraeCli => Some(AutomationAgent::TraeCli),
            Self::Claude => Some(AutomationAgent::Claude),
            _ => None,
        }
    }
}

impl AgentAutomation for AutomationAgent {
    fn session_id_mode(&self) -> SessionIdMode {
        match self {
            Self::Codex => codex::Codex.session_id_mode(),
            Self::TraeCli => traecli::TraeCli.session_id_mode(),
            Self::Claude => claude::Claude.session_id_mode(),
        }
    }
    fn validate_arguments(&self, arguments: &[String]) -> Result<()> {
        match self {
            Self::Codex => codex::Codex.validate_arguments(arguments),
            Self::TraeCli => traecli::TraeCli.validate_arguments(arguments),
            Self::Claude => claude::Claude.validate_arguments(arguments),
        }
    }
    fn automation_arguments(&self, yolo: bool, session_id: Option<&str>) -> Result<Vec<String>> {
        match self {
            Self::Codex => codex::Codex.automation_arguments(yolo, session_id),
            Self::TraeCli => traecli::TraeCli.automation_arguments(yolo, session_id),
            Self::Claude => claude::Claude.automation_arguments(yolo, session_id),
        }
    }
    fn session_from_line(&self, line: &[u8]) -> Option<String> {
        match self {
            Self::Codex => codex::Codex.session_from_line(line),
            Self::TraeCli => traecli::TraeCli.session_from_line(line),
            Self::Claude => claude::Claude.session_from_line(line),
        }
    }
}

pub(crate) fn validate_common_arguments(arguments: &[String]) -> Result<()> {
    // These options would violate the new-session and native-persistence contract.
    for argument in arguments {
        let key = argument.split('=').next().unwrap_or(argument);
        ensure!(
            ![
                "resume",
                "fork",
                "exec",
                "--resume",
                "--continue",
                "--fork-session",
                "--ephemeral",
                "--no-session-persistence",
                "--session-id",
                "--output-format",
                "--json",
                "--print",
                "--from-pr",
                "--teleport",
                "--background",
                "--bg",
                "--cloud",
                "--worktree",
                "--cd",
                "-C",
                "-w"
            ]
            .contains(&key),
            "Agent 启动参数与自动化执行模式冲突: {key}"
        );
    }
    Ok(())
}

/// Configuration paths captured at task creation so native history stays discoverable.
pub fn configuration_environment() -> std::collections::BTreeMap<String, String> {
    crate::KNOWN_AGENTS
        .iter()
        .flat_map(|agent| agent.definition().configuration_env)
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests;
