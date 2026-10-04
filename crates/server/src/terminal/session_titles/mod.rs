//! Display titles for agents whose CLI does not publish session titles via OSC.

use aow_agents::{
    Agent,
    sessions::{titles::AgentSessionTitleProvider, tracking::LiveSessionContext},
};
use aow_protocol::{TerminalAgentList, TerminalAgentProcess};

use super::sessions;

pub(super) async fn enrich(detected: &mut TerminalAgentList) {
    for (pane, agent) in &detected.agents {
        let Some(provider) = agent
            .as_deref()
            .and_then(Agent::from_id)
            .and_then(Agent::session_titles)
        else {
            continue;
        };
        let Some(process) = detected.processes.get(pane) else {
            continue;
        };
        let terminal_title = detected.titles.get(pane).map_or("", String::as_str);
        if let Some(title) = native_title(&provider, process.clone(), terminal_title).await {
            detected.titles.insert(pane.clone(), title);
        }
    }
}

async fn native_title(
    provider: &impl AgentSessionTitleProvider,
    process: TerminalAgentProcess,
    terminal_title: &str,
) -> Option<String> {
    let identity = process.clone();
    let environment = tokio::task::spawn_blocking(move || sessions::process_environment(&identity))
        .await
        .ok()??;
    let title = provider
        .session_title(LiveSessionContext {
            pid: Some(process.pid),
            cwd: &process.cwd,
            title: terminal_title,
            environment: &environment,
        })
        .await?;
    sessions::same_process(&process).then_some(title)
}

#[cfg(test)]
mod tests;
