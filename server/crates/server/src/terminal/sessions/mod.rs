//! On-demand, read-only association of a terminal pane with native agent history.

use super::*;
#[cfg(test)]
use aow_agents::sessions::{
    AgentSession, SessionRoots,
    tracking::{LiveSessionContext, SessionResolution, SessionTarget},
};
#[cfg(test)]
use aow_protocol::TerminalAgentProcess;

#[cfg(test)]
mod local;
#[cfg(test)]
mod process;
#[cfg(test)]
pub(super) use process::{process_environment, same_process};

#[cfg(test)]
#[derive(Serialize)]
pub(super) struct PaneSessions {
    agent: String,
    cwd: String,
    title: String,
    process: Option<TerminalAgentProcess>,
    /// The native session identity resolved by the agent adapter, if available.
    live_session_id: Option<String>,
    sessions: Vec<AgentSession>,
}

pub(super) async fn list(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let tab = state
        .terminals
        .get_snapshot(&tab_id)
        .map_err(terminal_http_error)?;
    let _pane = tab
        .panes
        .iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| terminal_http_error(TerminalError::PaneNotFound(pane_id.clone())))?;
    if state.acp.runtime().is_some() {
        let result = state
            .terminals
            .inner
            .terminald
            .native_sessions(&pane_id)
            .await
            .map_err(|error| terminal_http_error(map_client_error(error)))?;
        for session in &result.sessions {
            state
                .aow
                .cache_native_sessions(
                    &session.reference.cwd,
                    Some(&result.agent),
                    std::slice::from_ref(session),
                    false,
                )
                .map_err(crate::aow::aow_http_error)?;
        }
        return serde_json::to_value(result)
            .map(Json)
            .map_err(|error| HttpError::internal(error.to_string()));
    }
    #[cfg(test)]
    {
        local::list(state, tab, pane_id).await
    }
    #[cfg(not(test))]
    {
        Err(HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_unavailable",
            "Native session service is unavailable",
            None,
        ))
    }
}
