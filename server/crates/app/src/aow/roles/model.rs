use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RoleRuntime {
    Acp { agent_id: String },
    Cli { agent_id: String },
}

pub(crate) use aow_runtime_contract::environment::{HomeLayout, Identity, IdentityPreset};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub runtime: RoleRuntime,
    #[serde(default)]
    pub home_layout: HomeLayout,
    pub identity: Identity,
    #[serde(default)]
    pub instruction: String,
    /// Absolute directories containing SKILL.md, copied into the role's home.
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AgentRole {
    pub id: String,
    #[serde(flatten)]
    pub config: RoleConfig,
}
