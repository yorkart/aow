#[derive(Clone, Copy)]
pub struct AgentDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub commands: &'static [&'static str],
    /// Native configuration locations, safe to pass to scheduled executions.
    pub configuration_env: &'static [&'static str],
    pub args: &'static [&'static str],
}

pub const CLAUDE: AgentDefinition = AgentDefinition {
    id: "claude",
    display_name: "Claude Code",
    commands: &["claude"],
    args: &[],
    configuration_env: &["CLAUDE_CONFIG_DIR"],
};

pub const CODEX: AgentDefinition = AgentDefinition {
    id: "codex",
    display_name: "Codex",
    commands: &["codex"],
    args: &[],
    configuration_env: &["CODEX_HOME"],
};

pub const TRAECLI: AgentDefinition = AgentDefinition {
    id: "traecli",
    display_name: "TraeCode CLI",
    commands: &["traecli"],
    args: &[],
    configuration_env: &["TRAECLI_HOME", "TRAE_HOME"],
};

/// Built-in agent adapters. New behavior is added in source and compiled into the binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Agent {
    Claude,
    Codex,
    TraeCli,
    Gemini,
    OpenCode,
    Aider,
    Goose,
    Cursor,
    Copilot,
    Kimi,
    QwenCode,
    Kiro,
}

pub const KNOWN_AGENTS: &[Agent] = &[
    Agent::Claude,
    Agent::Codex,
    Agent::TraeCli,
    Agent::Gemini,
    Agent::OpenCode,
    Agent::Aider,
    Agent::Goose,
    Agent::Cursor,
    Agent::Copilot,
    Agent::Kimi,
    Agent::QwenCode,
    Agent::Kiro,
];

impl Agent {
    pub fn from_id(id: &str) -> Option<Self> {
        KNOWN_AGENTS
            .iter()
            .copied()
            .find(|agent| agent.definition().id == id)
    }

    pub fn id(self) -> &'static str {
        self.definition().id
    }

    pub fn definition(self) -> &'static AgentDefinition {
        match self {
            Self::Claude => &CLAUDE,
            Self::Codex => &CODEX,
            Self::TraeCli => &TRAECLI,
            Self::Gemini => &AgentDefinition {
                id: "gemini",
                display_name: "Gemini CLI",
                commands: &["gemini"],
                configuration_env: &[],
                args: &[],
            },
            Self::OpenCode => &AgentDefinition {
                id: "opencode",
                display_name: "OpenCode",
                commands: &["opencode"],
                configuration_env: &[],
                args: &[],
            },
            Self::Aider => &AgentDefinition {
                id: "aider",
                display_name: "Aider",
                commands: &["aider"],
                configuration_env: &[],
                args: &[],
            },
            Self::Goose => &AgentDefinition {
                id: "goose",
                display_name: "Goose",
                commands: &["goose"],
                configuration_env: &[],
                args: &[],
            },
            Self::Cursor => &AgentDefinition {
                id: "cursor",
                display_name: "Cursor Agent",
                commands: &["cursor-agent"],
                configuration_env: &[],
                args: &[],
            },
            Self::Copilot => &AgentDefinition {
                id: "copilot",
                display_name: "GitHub Copilot",
                commands: &["copilot"],
                configuration_env: &[],
                args: &[],
            },
            Self::Kimi => &AgentDefinition {
                id: "kimi",
                display_name: "Kimi CLI",
                commands: &["kimi"],
                configuration_env: &[],
                args: &[],
            },
            Self::QwenCode => &AgentDefinition {
                id: "qwen-code",
                display_name: "Qwen Code",
                commands: &["qwen"],
                configuration_env: &[],
                args: &[],
            },
            Self::Kiro => &AgentDefinition {
                id: "kiro",
                display_name: "Kiro CLI",
                commands: &["kiro-cli"],
                configuration_env: &[],
                args: &["chat", "--tui"],
            },
        }
    }
}
