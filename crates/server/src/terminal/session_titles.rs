//! Display titles for agents whose CLI does not publish session titles via OSC.

use aow_agents::{
    Agent,
    sessions::{
        SessionRoots,
        tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
    },
};
use aow_protocol::{TerminalAgentList, TerminalAgentProcess};

use super::sessions;

pub(super) async fn enrich(detected: &mut TerminalAgentList) {
    for (pane, agent) in &detected.agents {
        // Keep other agents' native activity/attention titles. Hermes resolves
        // identity from its PID registry, never from this display-only title.
        if agent.as_deref() != Some(Agent::Hermes.id()) {
            continue;
        }
        let Some(process) = detected.processes.get(pane) else {
            continue;
        };
        if let Some(title) = hermes_title(process.clone()).await {
            detected.titles.insert(pane.clone(), title);
        }
    }
}

async fn hermes_title(process: TerminalAgentProcess) -> Option<String> {
    let identity = process.clone();
    let environment = tokio::task::spawn_blocking(move || sessions::process_environment(&identity))
        .await
        .ok()??;
    let home = environment.get("HOME")?;
    let tracker = Agent::Hermes.session_tracking()?;
    let SessionResolution::Resolved(target @ SessionTarget::Id(_)) = tracker
        .resolve_live_session(LiveSessionContext {
            pid: Some(process.pid),
            cwd: &process.cwd,
            title: "",
            environment: &environment,
        })
        .await
    else {
        return None;
    };
    let roots = SessionRoots::from_configuration(home, &environment);
    tokio::task::spawn_blocking(move || {
        let candidates = tracker.candidate_sessions(&target, process.cwd.as_ref(), roots);
        // Refresh from the native database on each metadata poll: generated
        // titles, /title and /new must not require a completion event or restart.
        let [session] = candidates.as_slice() else {
            return None;
        };
        sessions::same_process(&process).then(|| session.title.clone())
    })
    .await
    .ok()?
}

#[cfg(test)]
mod tests;
