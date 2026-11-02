use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RoleRuntime {
    Acp { agent_id: String },
    Cli { agent_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Identity {
    LocalHome { source: Option<String> },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct IdentityPreset {
    pub layout: HomeLayout,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HomeLayout {
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

impl HomeLayout {
    pub(super) fn directory(self) -> &'static str {
        match self {
            Self::Codex => ".codex",
            Self::Claude => ".claude",
            Self::Pi => ".pi/agent",
            Self::Hermes => ".hermes",
            Self::Traecli => ".trae",
            Self::Gemini => ".gemini",
            Self::Opencode => ".local/share/opencode",
            Self::Custom => ".",
        }
    }

    pub(super) fn skills_directory(self) -> std::path::PathBuf {
        // OpenCode credentials live in XDG data, but its skills live in config.
        std::path::Path::new(if self == Self::Opencode {
            ".config/opencode"
        } else {
            self.directory()
        })
        .join("skills")
    }
}

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
