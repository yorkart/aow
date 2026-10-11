//! Resolved launch configuration shared by terminal callers.
use crate::Agent;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub use crate::codex_terminal::prepare as prepare_codex_terminal;

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

    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
            Self::Hermes => Agent::Hermes,
            Self::Pi => Agent::Pi,
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
    /// Keep managed Codex TUI writers in the pane process, without changing its title.
    pub async fn prepare_terminal(&mut self, pane_id: &str, cwd: &Path) {
        if self.agent_type == AgentType::Codex {
            prepare_codex_terminal(
                &self.executable,
                pane_id,
                cwd,
                &mut self.args,
                &mut self.env,
            )
            .await;
        }
    }

    /// Resume a native session with this instance's configured command and environment.
    pub fn resume_session(&mut self, session_id: &str) -> Result<(), LaunchError> {
        if session_id.trim().is_empty()
            || session_id.starts_with('-')
            || session_id.chars().any(char::is_control)
        {
            return Err(LaunchError::Invalid("invalid resume session ID".to_owned()));
        }
        let argument = match self.agent_type {
            AgentType::Claude | AgentType::Hermes => "--resume",
            AgentType::Codex | AgentType::TraeCli => "resume",
            AgentType::Pi => "--session",
        };
        self.args
            .extend([argument.to_owned(), session_id.to_owned()]);
        Ok(())
    }
}

impl AgentRegistration {
    /// Identify legacy terminal profiles after AoW adds isolation/resume arguments.
    pub fn matches_terminal_arguments(&self, arguments: &[String]) -> bool {
        let Some(kind) = self.agent_type else {
            return false;
        };
        let comparable = |arguments: &[String]| {
            let borrowed: Vec<_> = arguments.iter().map(String::as_str).collect();
            let injected = (kind == AgentType::Codex)
                .then(|| crate::codex_command::flags(&borrowed).no_daemon)
                .flatten();
            arguments
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != injected)
                .map(|(_, arg)| arg.clone())
                .collect::<Vec<_>>()
        };
        let profile = comparable(&self.args);
        let terminal = comparable(arguments);
        if terminal == profile {
            return true;
        }
        let suffix = terminal.strip_prefix(profile.as_slice());
        let resume = match kind {
            AgentType::Claude | AgentType::Hermes => "--resume",
            AgentType::Pi => "--session",
            AgentType::Codex | AgentType::TraeCli => "resume",
        };
        matches!(suffix, Some([flag, session]) if flag == resume && !session.is_empty())
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(agent_type: AgentType, args: &[&str]) -> AgentLaunch {
        AgentLaunch {
            agent_type,
            display_name: "test".into(),
            executable: "codex".into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            env: Default::default(),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_terminal_isolation_preserves_profiles_resume_and_literal_prompts() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("codex");
        std::fs::write(&executable, "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --help ] || exit 1\nprintf '%s\\n' '  --no-daemon'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut launch = launch(AgentType::Codex, &["--profile", "work"]);
        launch.executable = executable.to_string_lossy().into_owned();
        launch.resume_session("session-a").unwrap();
        launch.prepare_terminal("pane-a", directory.path()).await;
        launch.prepare_terminal("pane-a", directory.path()).await;
        assert_eq!(
            launch.args,
            ["--profile", "work", "resume", "session-a", "--no-daemon"]
        );
        assert_eq!(launch.env[crate::codex_command::TERMINAL_ENV], "pane-a");
        let mut launch = self::launch(AgentType::Codex, &["--", "--no-daemon"]);
        launch.executable = executable.to_string_lossy().into_owned();
        launch.prepare_terminal("pane-a", directory.path()).await;
        assert_eq!(launch.args, ["--no-daemon", "--", "--no-daemon"]);
    }

    #[tokio::test]
    async fn explicit_remote_and_other_agents_keep_their_launch_arguments() {
        for args in [
            vec!["--remote", "unix:///tmp/server"],
            vec!["--remote=ws://localhost:1234"],
        ] {
            let mut launch = launch(AgentType::Codex, &args);
            launch.prepare_terminal("pane", Path::new(".")).await;
            assert_eq!(launch.args, args);
            assert!(!launch.env.contains_key(crate::codex_command::TERMINAL_ENV));
        }
        for agent in [
            AgentType::Claude,
            AgentType::TraeCli,
            AgentType::Pi,
            AgentType::Hermes,
        ] {
            let mut launch = launch(agent, &[]);
            launch.prepare_terminal("pane", Path::new(".")).await;
            assert!(launch.args.is_empty());
            assert!(launch.env.is_empty());
        }
    }

    #[test]
    fn legacy_codex_profile_matching_ignores_only_the_effective_isolation_flag() {
        let registration = AgentRegistration {
            id: "work".into(),
            agent_type: Some(AgentType::Codex),
            display_name: "work".into(),
            source: "custom",
            available: true,
            command: "codex".into(),
            executable: Some("/opt/codex".into()),
            args: vec!["--profile".into(), "work".into()],
            env: Default::default(),
        };
        for arguments in [
            vec!["--profile", "work"],
            vec!["--profile", "work", "--no-daemon"],
            vec!["--profile", "work", "resume", "session", "--no-daemon"],
        ] {
            let args: Vec<_> = arguments.into_iter().map(String::from).collect();
            assert!(registration.matches_terminal_arguments(&args), "{args:?}");
        }
        for arguments in [
            vec!["--profile", "other", "--no-daemon"],
            vec!["--profile", "work", "--", "--no-daemon"],
            vec!["--profile", "work", "-c", "--no-daemon"],
        ] {
            let args: Vec<_> = arguments.into_iter().map(String::from).collect();
            assert!(!registration.matches_terminal_arguments(&args), "{args:?}");
        }
        let registration = AgentRegistration {
            args: vec!["--".into(), "prompt".into()],
            ..registration
        };
        assert!(registration.matches_terminal_arguments(&[
            "--no-daemon".into(),
            "--".into(),
            "prompt".into(),
        ]));
    }
}
