use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Supported registration types, independent of a configuration's unique ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    TraeCli,
    Hermes,
    Pi,
}

impl AgentType {
    pub const ALL: [Self; 5] = [
        Self::Claude,
        Self::Codex,
        Self::TraeCli,
        Self::Hermes,
        Self::Pi,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::TraeCli => "traecli",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
        }
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistration {
    pub is_default: bool,
    pub id: String,
    /// None only for legacy registrations that still need a type selected.
    pub agent_type: Option<AgentType>,
    pub display_name: String,
    pub source: String,
    pub available: bool,
    pub command: String,
    pub executable: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLaunch {
    pub agent_type: AgentType,
    pub display_name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub id: String,
    pub display_name: String,
    pub commands: Vec<String>,
    pub configuration_env: Vec<String>,
    pub interactive: bool,
    pub automation: bool,
    pub sessions: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}
