//! On-demand, read-only association of a terminal pane with native agent history.

use super::*;
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
    {
        let result = state
            .terminals
            .inner
            .terminald
            .native_sessions(&pane_id)
            .await
            .map_err(|error| terminal_http_error(map_client_error(error)))?;
        for session in &result.sessions {
            state
                .agents
                .cache_native_sessions(
                    &session.reference.cwd,
                    Some(&result.agent),
                    std::slice::from_ref(session),
                    false,
                )
                .map_err(crate::error::service_http_error)?;
        }
        crate::agents::sessions::public_native_sessions(result)
            .map(Json)
            .map_err(|error| HttpError::internal(error.to_string()))
    }
}
