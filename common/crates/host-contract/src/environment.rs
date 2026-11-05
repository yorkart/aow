use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Identity {
    LocalHome { source: Option<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityPreset {
    pub layout: HomeLayout,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HomeLayout {
    Codex,
    Claude,
    Pi,
    Hermes,
    Traecli,
    Gemini,
    Opencode,
    #[default]
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RoleEnvironmentSpec {
    pub id: String,
    pub home_layout: HomeLayout,
    pub identity: Identity,
    #[serde(default)]
    pub skills: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparedEnvironment {
    pub home: std::path::PathBuf,
    pub env: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrepareEnvironment {
    pub spec: RoleEnvironmentSpec,
    #[serde(default)]
    pub published: bool,
    pub token: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinishEnvironment {
    pub token: String,
    pub spec: RoleEnvironmentSpec,
    pub commit: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentPreparation {
    pub token: String,
    #[serde(flatten)]
    pub environment: PreparedEnvironment,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeEnvironment {
    pub spec: RoleEnvironmentSpec,
    pub launch: Option<crate::agents::AgentLaunch>,
    pub acp_agent_id: Option<String>,
    #[serde(default = "probe_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeEnvironment {
    pub state_dir: std::path::PathBuf,
    pub execution_path: Vec<std::path::PathBuf>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentUpdate {
    pub execution_path: Vec<std::path::PathBuf>,
}

fn probe_timeout() -> u64 {
    60_000
}
