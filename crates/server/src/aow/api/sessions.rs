use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/agent-sessions", get(list_agent_sessions))
        .route(
            "/api/aow/agent-sessions/automation-run",
            get(automation_run_session),
        )
        .route(
            "/api/aow/agent-sessions/{session_id}/snapshot",
            get(agent_session_snapshot),
        )
}
async fn list_agent_sessions(
    State(state): State<AppState>,
    Query(query): Query<AgentSessionsQuery>,
) -> Result<Json<Vec<aow_agents::sessions::AgentSession>>, Response> {
    let worktree = state
        .aow
        .registered_worktree_path(&query.worktree_path)
        .await
        .map_err(aow_response)?;
    let roots = aow_agents::sessions::SessionRoots::from_environment(&crate::PROCESS_HOME);
    let agent = query.agent;
    if agent.as_deref().is_some_and(|agent| {
        aow_agents::Agent::from_id(agent)
            .and_then(aow_agents::Agent::sessions)
            .is_none()
    }) {
        return Err(aow_response(AowError::Invalid(
            "unsupported agent session source".to_owned(),
        )));
    }
    let list_worktree = worktree.clone();
    let list_agent = agent.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        aow_agents::sessions::list_sessions(&list_worktree, list_agent.as_deref(), roots)
    })
    .await
    .map_err(|error| {
        HttpError::internal(format!("agent session scan failed: {error}")).into_response()
    })?;
    state
        .aow
        .replace_session_locators_for_agent(&worktree, agent.as_deref(), &sessions)
        .map_err(aow_response)?;
    Ok(Json(sessions))
}

async fn automation_run_session(
    State(state): State<AppState>,
    Query(query): Query<AutomationRunSessionQuery>,
) -> Result<Json<aow_agents::sessions::AgentSession>, Response> {
    let automations = state.automations.as_ref().ok_or_else(|| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "automations_unavailable",
            "automations require persistent state",
            None,
        )
        .into_response()
    })?;
    let run = automations
        .run(&query.task_id, &query.run_id)
        .map_err(|error| aow_response(AowError::Invalid(error.to_string())))?;
    let session_id = run.session_id.ok_or_else(|| {
        aow_response(AowError::Invalid(
            "automation run has no agent session".to_owned(),
        ))
    })?;
    let agent = run.agent.id();
    let roots = aow_agents::sessions::SessionRoots::from_environment(&crate::PROCESS_HOME);
    let found_agent = agent.to_owned();
    let found_session_id = session_id.clone();
    let session = tokio::task::spawn_blocking(move || {
        aow_agents::sessions::find_session(&found_agent, &found_session_id, roots)
    })
    .await
    .map_err(|error| {
        HttpError::internal(format!("automation agent session lookup failed: {error}"))
            .into_response()
    })?
    .ok_or_else(|| {
        HttpError::new(
            StatusCode::NOT_FOUND,
            "agent_session_not_found",
            "agent session is no longer available",
            None,
        )
        .into_response()
    })?;
    state
        .aow
        .cache_session_locator(&session.cwd_path(), &session)
        .map_err(aow_response)?;
    Ok(Json(session))
}

async fn agent_session_snapshot(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AgentSessionSnapshotQuery>,
) -> Result<Json<aow_agents::sessions::snapshot::AgentSessionSnapshot>, Response> {
    let locator =
        resolve_session_locator(&state, &session_id, &query.agent, &query.worktree_path).await?;
    tokio::task::spawn_blocking(move || aow_agents::sessions::snapshot::read(locator))
        .await
        .map_err(|error| {
            HttpError::internal(format!("agent session snapshot task failed: {error}"))
                .into_response()
        })?
        .map(Json)
        .map_err(snapshot_response)
}

pub(crate) async fn resolve_session_locator(
    state: &AppState,
    session_id: &str,
    agent: &str,
    worktree_path: &str,
) -> Result<aow_agents::sessions::AgentSessionLocator, Response> {
    if aow_agents::Agent::from_id(agent)
        .and_then(aow_agents::Agent::sessions)
        .is_none()
        || session_id.is_empty()
        || session_id.len() > 256
    {
        return Err(aow_response(AowError::Invalid(
            "invalid agent session identity".to_owned(),
        )));
    }
    // A locator is only cached by a registered-session scan or a trusted
    // automation run lookup. Once one exists, the exact workspace key is
    // enough to read its snapshot, including an automation-created worktree
    // that is intentionally not registered as a project worktree.
    let requested = Path::new(worktree_path);
    let worktree = match paths::canonical_directory(requested).await {
        Ok(worktree) => worktree,
        Err(AowError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            super::super::sessions::removed_directory_path(requested).map_err(aow_response)?
        }
        Err(error) => return Err(aow_response(error)),
    };
    state
        .aow
        .session_locator(&worktree, agent, session_id)
        .map_err(aow_response)
}
