//! Explicit in-process fixtures; production only consumes node session APIs.
use super::*;
pub(super) async fn list(
    state: AppState,
    tab: TerminalTab,
    pane_id: String,
) -> Result<Json<serde_json::Value>, HttpError> {
    let pane = tab.panes.iter().find(|pane| pane.id == pane_id).unwrap();
    let detected = state
        .terminals
        .agents(Some(&tab.workspace_root))
        .await
        .map_err(terminal_http_error)?;
    let agent = detected
        .agents
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| pane.agent_id.clone())
        .filter(|_| pane.status == TerminalPaneStatus::Running)
        .filter(|id| {
            aow_agents::Agent::from_id(id)
                .and_then(aow_agents::Agent::sessions)
                .is_some()
        })
        .ok_or_else(|| {
            terminal_http_error(TerminalError::Conflict(
                aow_i18n::text("server.no_agent_in_this_panel_supports_reading_sessions").into(),
            ))
        })?;
    let process = detected.processes.get(&pane_id).cloned();
    let cwd = process
        .as_ref()
        .map_or_else(|| pane.cwd.clone(), |process| process.cwd.clone());
    let title = detected.titles.get(&pane_id).cloned().unwrap_or_default();
    let environment = process.as_ref().and_then(process_environment);
    // A process may exit between the daemon scan and this request. Never query
    // another process after PID reuse or use the server's config for that PID.
    if process.is_some() && environment.is_none() {
        return Err(terminal_http_error(TerminalError::Conflict(
            aow_i18n::text("server.agent_process_changed_refresh_the_session_list").into(),
        )));
    }
    let roots = environment.as_ref().map_or_else(
        || SessionRoots::from_environment(&crate::PROCESS_HOME),
        |environment| {
            SessionRoots::from_configuration(
                environment
                    .get("HOME")
                    .map_or(crate::PROCESS_HOME.as_path(), PathBuf::as_path),
                environment,
            )
        },
    );
    let tracker = aow_agents::Agent::from_id(&agent).and_then(aow_agents::Agent::session_tracking);
    let live_session_id = if let (Some(tracker), Some(process), Some(environment)) =
        (tracker, &process, &environment)
    {
        match tracker
            .resolve_live_session_with_timeout(
                LiveSessionContext {
                    pid: Some(process.pid),
                    cwd: &cwd,
                    title: &title,
                    environment,
                },
                state.terminals.inner.session_query_timeout,
            )
            .await
        {
            SessionResolution::Resolved(SessionTarget::Id(id)) => Some(id),
            _ => None,
        }
    } else {
        None
    };
    let scan_cwd = PathBuf::from(&cwd);
    let scan_agent = agent.clone();
    let exact_id = live_session_id.clone();
    let sessions = aow_i18n::spawn_blocking(move || {
        let mut sessions =
            aow_agents::sessions::list_sessions(&scan_cwd, Some(&scan_agent), roots.clone());
        if let Some(id) = exact_id
            && !sessions
                .iter()
                .any(|session| session.locator().session_id == id)
            && let Some(session) = aow_agents::sessions::find_session(&scan_agent, &id, roots)
        {
            sessions.insert(0, session);
        }
        sessions
    })
    .await
    .map_err(|error| HttpError::internal(format!("agent session scan failed: {error}")))?;
    if process
        .as_ref()
        .is_some_and(|process| !same_process(process))
    {
        return Err(terminal_http_error(TerminalError::Conflict(
            aow_i18n::text("server.agent_process_changed_refresh_the_session_list").into(),
        )));
    }
    // The pane supplies the trusted scan scope, including directories opened
    // with `cd` that aren't registered project worktrees. Snapshot reads still
    // go through the existing provider's transcript-root validation.
    for session in &sessions {
        let cwd = tokio::fs::canonicalize(session.cwd_path())
            .await
            .unwrap_or_else(|_| session.cwd_path());
        state
            .aow
            .cache_session_locator(&cwd, session)
            .map_err(crate::aow::aow_http_error)?;
    }
    serde_json::to_value(PaneSessions {
        agent,
        cwd,
        title,
        process,
        live_session_id,
        sessions,
    })
    .map(Json)
    .map_err(|error| HttpError::internal(error.to_string()))
}
