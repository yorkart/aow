use super::*;
use crate::CODEX;

pub(super) struct Codex;

impl AgentSessionProvider for Codex {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        environment
            .get("CODEX_HOME")
            .cloned()
            .unwrap_or_else(|| process_home.join(".codex"))
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        super::codex_like::CodexLikeSessionProvider {
            definition: &CODEX,
            home: roots.codex.clone(),
        }
        .list_sessions(workspace_path)
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        super::codex_like::CodexLikeSessionProvider {
            definition: &CODEX,
            home: roots.codex.clone(),
        }
        .find_session(session_id)
    }
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        super::codex_like::current_title(&CODEX, locator)
    }
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_codex_like(locator)
    }
}
