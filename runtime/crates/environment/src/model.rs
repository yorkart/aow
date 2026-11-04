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

impl HomeLayout {
    pub(crate) fn directory(self) -> &'static str {
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

    pub(crate) fn skills_directory(self) -> std::path::PathBuf {
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
