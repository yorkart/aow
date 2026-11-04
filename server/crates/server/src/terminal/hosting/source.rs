use super::*;
use anyhow::{Context, Result};
use aow_protocol::TerminalAgentProcess;
pub(super) struct Source {
    pub agent: String,
    pub session_id: String,
    pub process: TerminalAgentProcess,
}
pub(super) async fn read(manager: &TerminalManager, pane_id: &str) -> Result<Source> {
    let sessions = manager.inner.terminald.native_sessions(pane_id).await?;
    let reference = sessions
        .current
        .context("Current agent session cannot be uniquely identified")?;
    let process = sessions
        .process
        .context("Current agent process is unavailable")?;
    Ok(Source {
        agent: reference.agent,
        session_id: reference.session_id,
        process,
    })
}
pub(super) fn matches(hosting: &TerminalHosting, source: &Source) -> bool {
    hosting.agent == source.agent
        && hosting.session_id == source.session_id
        && hosting.process == source.process
}
