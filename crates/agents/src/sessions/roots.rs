use std::path::{Path, PathBuf};

use super::{
    helpers::environment_path,
    model::{AgentSessionProvider, SessionEnvironment},
    provider::SessionAgent,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionRoots {
    pub(crate) claude: PathBuf,
    pub(crate) codex: PathBuf,
    pub(crate) traecli: PathBuf,
    pub(crate) hermes: PathBuf,
    #[serde(default)]
    pub(crate) pi: PathBuf,
}

impl SessionRoots {
    pub fn from_environment(process_home: &Path) -> Self {
        let environment = crate::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env)
            .filter_map(|key| environment_path(key).map(|path| ((*key).to_owned(), path)))
            .collect();
        Self::from_configuration(process_home, &environment)
    }

    pub fn from_configuration(process_home: &Path, environment: &SessionEnvironment) -> Self {
        let process_home = environment
            .get("HOME")
            .filter(|home| !home.as_os_str().is_empty())
            .map(PathBuf::as_path)
            .unwrap_or(process_home);
        Self {
            claude: SessionAgent::Claude.session_root(process_home, environment),
            codex: SessionAgent::Codex.session_root(process_home, environment),
            traecli: SessionAgent::TraeCli.session_root(process_home, environment),
            hermes: SessionAgent::Hermes.session_root(process_home, environment),
            pi: SessionAgent::Pi.session_root(process_home, environment),
        }
    }

    #[cfg(test)]
    pub(super) fn from_values(
        process_home: &Path,
        claude: Option<PathBuf>,
        codex: Option<PathBuf>,
        traecli: Option<PathBuf>,
        trae: Option<PathBuf>,
    ) -> Self {
        let environment = [
            ("CLAUDE_CONFIG_DIR", claude),
            ("CODEX_HOME", codex),
            ("TRAECLI_HOME", traecli),
            ("TRAE_HOME", trae),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
        .collect();
        Self::from_configuration(process_home, &environment)
    }
}
