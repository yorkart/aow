use super::*;

pub(super) struct TraeCli;

impl AgentSessionTracker for TraeCli {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        codex_like::resolve(context)
    }

    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator> {
        codex_like::candidates(Agent::TraeCli, target, cwd, roots)
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        Box::new(codex_like::Parser::default())
    }
}
