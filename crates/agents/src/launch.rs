//! Resolved launch configuration shared by terminal callers.
use crate::Agent;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("agent not found or unavailable: {0}")]
    Unavailable(String),
    #[error("{0}")]
    Invalid(String),
}

/// Supported registration types, independent of a configuration's unique ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    TraeCli,
}

impl AgentType {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::TraeCli];

    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
        }
    }

    pub fn id(self) -> &'static str {
        self.agent().id()
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|agent_type| agent_type.id() == id)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRegistration {
    pub id: String,
    /// None only for legacy registrations that still need a type selected.
    pub agent_type: Option<AgentType>,
    pub display_name: String,
    pub source: &'static str,
    pub available: bool,
    pub command: String,
    pub executable: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub struct AgentLaunch {
    pub agent_type: AgentType,
    pub display_name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl AgentLaunch {
    /// Resume a native session with this instance's configured command and environment.
    pub fn resume_session(&mut self, session_id: &str) -> Result<(), LaunchError> {
        if session_id.trim().is_empty()
            || session_id.starts_with('-')
            || session_id.chars().any(char::is_control)
        {
            return Err(LaunchError::Invalid("invalid resume session ID".to_owned()));
        }
        let argument = match self.agent_type {
            AgentType::Claude => "--resume",
            AgentType::Codex | AgentType::TraeCli => "resume",
        };
        self.args
            .extend([argument.to_owned(), session_id.to_owned()]);
        Ok(())
    }
}

impl AgentRegistration {
    pub fn into_launch(self, paths: &[PathBuf]) -> Result<AgentLaunch, LaunchError> {
        let agent_type = self.agent_type.ok_or_else(|| {
            LaunchError::Invalid(format!("请先为 {} 设置 Agent 类型", self.display_name))
        })?;
        let executable = self
            .executable
            .ok_or_else(|| LaunchError::Unavailable(self.id.clone()))?;
        // terminald may run with a sparse service PATH. Forward the same search
        // path used for discovery so /usr/bin/env shebangs and child tools work.
        let path = std::env::join_paths(paths)
            .map_err(|error| LaunchError::Invalid(format!("invalid agent PATH: {error}")))?;
        // terminald inherits its process environment and applies these additions
        // and overrides. The shared execution PATH remains authoritative.
        let mut env = self.env;
        env.insert("PATH".to_owned(), path.to_string_lossy().into_owned());
        Ok(AgentLaunch {
            agent_type,
            display_name: self.display_name,
            executable,
            args: self.args,
            env,
        })
    }
}
