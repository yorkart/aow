//! Resolved launch configuration shared by terminal callers.
use crate::Agent;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use crate::codex_terminal::prepare as prepare_codex_terminal;

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("{translated}", translated = aow_i18n::message("errors.agent_not_found_or_unavailable", &[("p0", .0.to_string())]))]
    Unavailable(String),
    #[error("{0}")]
    Invalid(String),
}

pub use aow_runtime_contract::agents::{AgentLaunch, AgentRegistration, AgentType};
pub trait AgentTypeExt {
    fn agent(self) -> Agent;
}
impl AgentTypeExt for AgentType {
    fn agent(self) -> Agent {
        Agent::from_id(self.id()).unwrap()
    }
}
pub trait AgentLaunchExt {
    fn prepare_terminal(
        &mut self,
        pane_id: &str,
        cwd: &Path,
    ) -> impl std::future::Future<Output = ()> + Send;
    fn resume_session(&mut self, session_id: &str) -> Result<(), LaunchError>;
}
pub trait AgentRegistrationExt {
    fn matches_terminal_arguments(&self, arguments: &[String]) -> bool;
    fn into_launch(self, paths: &[PathBuf]) -> Result<AgentLaunch, LaunchError>;
}
impl AgentLaunchExt for AgentLaunch {
    /// Keep managed Codex TUI writers in the pane process, without changing its title.
    async fn prepare_terminal(&mut self, pane_id: &str, cwd: &Path) {
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
    fn resume_session(&mut self, session_id: &str) -> Result<(), LaunchError> {
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

impl AgentRegistrationExt for AgentRegistration {
    /// Identify legacy terminal profiles after AoW adds isolation/resume arguments.
    fn matches_terminal_arguments(&self, arguments: &[String]) -> bool {
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

    fn into_launch(self, paths: &[PathBuf]) -> Result<AgentLaunch, LaunchError> {
        let agent_type = self.agent_type.ok_or_else(|| {
            LaunchError::Invalid(aow_i18n::message(
                "agents.set_the_agent_type_for_first",
                &[("auto0", self.display_name.to_string())],
            ))
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
            is_default: false,
            id: "work".into(),
            agent_type: Some(AgentType::Codex),
            display_name: "work".into(),
            source: "custom".into(),
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
            is_default: false,
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
