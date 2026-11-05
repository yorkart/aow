pub use aow_host_contract::environment::{
    HomeLayout, Identity, IdentityPreset, PreparedEnvironment, RoleEnvironmentSpec,
};
pub(crate) trait HomeLayoutExt {
    fn directory(self) -> &'static str;
    fn skills_directory(self) -> std::path::PathBuf;
}
impl HomeLayoutExt for HomeLayout {
    fn directory(self) -> &'static str {
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

    fn skills_directory(self) -> std::path::PathBuf {
        // OpenCode credentials live in XDG data, but its skills live in config.
        std::path::Path::new(if self == Self::Opencode {
            ".config/opencode"
        } else {
            self.directory()
        })
        .join("skills")
    }
}
