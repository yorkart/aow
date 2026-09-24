use super::*;
use crate::TRAECLI;

pub(super) struct TraeCli;

impl AgentSessionProvider for TraeCli {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        // Public 2.0 release 0.207.1-tob: `doctor --no-network --json`
        // reports cli_home separately from the ~/.trae configuration root.
        environment.get("TRAECLI_HOME").cloned().unwrap_or_else(|| {
            environment
                .get("TRAE_HOME")
                .cloned()
                .unwrap_or_else(|| process_home.join(".trae"))
                .join("cli")
        })
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        super::codex_like::CodexLikeSessionProvider {
            definition: &TRAECLI,
            home: roots.traecli.clone(),
        }
        .list_sessions(workspace_path)
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        super::codex_like::CodexLikeSessionProvider {
            definition: &TRAECLI,
            home: roots.traecli.clone(),
        }
        .find_session(session_id)
    }
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        super::codex_like::current_title(&TRAECLI, locator)
    }
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_codex_like(locator)
    }
}
