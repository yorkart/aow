use super::*;
use crate::sessions::{
    SessionRoots,
    tracking::{AgentSessionTracker, SessionResolution, SessionTarget},
};

pub(super) struct Hermes;

impl AgentSessionTitleProvider for Hermes {
    async fn session_title(&self, context: LiveSessionContext<'_>) -> Option<String> {
        let home = context.environment.get("HOME")?;
        let tracker = Agent::Hermes.session_tracking()?;
        let SessionResolution::Resolved(target @ SessionTarget::Id(_)) = tracker
            .resolve_live_session(LiveSessionContext {
                title: "",
                ..context
            })
            .await
        else {
            return None;
        };
        let roots = SessionRoots::from_configuration(home, context.environment);
        let cwd = std::path::PathBuf::from(context.cwd);
        tokio::task::spawn_blocking(move || {
            // Generated titles, /title and /new must refresh independently of
            // completion events. Never infer identity from a title or directory.
            let candidates = tracker.candidate_sessions(&target, &cwd, roots);
            let [session] = candidates.as_slice() else {
                return None;
            };
            Some(session.title.clone())
        })
        .await
        .ok()?
    }
}

#[cfg(test)]
mod tests;
