#![expect(
    clippy::result_large_err,
    reason = "HTTP route errors return Axum responses directly without an extra heap allocation"
)]

use super::*;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agent-sessions", get(list_agent_sessions))
        .route(
            "/api/agent-sessions/automation-run",
            get(automation_run_session),
        )
        .route(
            "/api/agent-sessions/{session_id}/snapshot",
            get(agent_session_snapshot),
        )
}
async fn list_agent_sessions(
    State(state): State<AppState>,
    Query(query): Query<AgentSessionsQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let worktree = state
        .projects
        .registered_worktree_path(&query.worktree_path)
        .await
        .map_err(service_response)?;
    {
        let runtime = state.node.agent_node().await.map_err(service_response)?;
        let sessions = runtime
            .native_sessions(&worktree, query.agent.as_deref())
            .await
            .map_err(|error| {
                crate::error::service_response(ServiceError::Invalid(error.to_string()))
            })?;
        state
            .agents
            .cache_native_sessions(&worktree, query.agent.as_deref(), &sessions, true)
            .map_err(service_response)?;
        crate::agents::sessions::public_native_sessions(sessions)
            .map(Json)
            .map_err(|error| HttpError::internal(error.to_string()).into_response())
    }
}

async fn automation_run_session(
    State(state): State<AppState>,
    Query(query): Query<AutomationRunSessionQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let automations = state.automations.as_ref().ok_or_else(|| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "automations_unavailable",
            aow_i18n::text("http.persistent_automations"),
            None,
        )
        .into_response()
    })?;
    let run = automations
        .run(&query.task_id, &query.run_id)
        .map_err(|error| service_response(ServiceError::Invalid(error.to_string())))?;
    let session_id = run.session_id.ok_or_else(|| {
        service_response(ServiceError::Invalid(
            aow_i18n::text("validation.automation_run_has_no_agent_session").to_owned(),
        ))
    })?;
    let agent = run.agent.id();
    {
        let runtime = state.node.agent_node().await.map_err(service_response)?;
        let reference = aow_automations::runner::execution_reference(
            automations.store(),
            &query.task_id,
            &query.run_id,
        )
        .map_err(|error| service_response(ServiceError::Invalid(error.to_string())))?
        .and_then(|reference| reference.native_session);
        let session = match reference {
            Some(reference) => runtime.find_native_reference(&reference).await,
            None => runtime.find_native_session(agent, &session_id).await,
        }
        .map_err(|error| service_response(ServiceError::Invalid(error.to_string())))?;
        let cwd = session.reference.cwd.clone();
        state
            .agents
            .cache_native_sessions(&cwd, Some(agent), std::slice::from_ref(&session), false)
            .map_err(service_response)?;
        crate::agents::sessions::public_native_sessions(session)
            .map(Json)
            .map_err(|error| HttpError::internal(error.to_string()).into_response())
    }
}

async fn agent_session_snapshot(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AgentSessionSnapshotQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let locator =
        resolve_session_locator(&state, &session_id, &query.agent, &query.worktree_path).await?;
    crate::session_shares::read_snapshot(
        Some(&state.node.agent_node().await.map_err(service_response)?),
        locator,
    )
    .await
    .map(Json)
    .map_err(IntoResponse::into_response)
}

pub(crate) async fn resolve_session_locator(
    state: &AppState,
    session_id: &str,
    agent: &str,
    worktree_path: &str,
) -> Result<crate::agents::sessions::SessionReference, Response> {
    if agent.is_empty() || session_id.is_empty() || session_id.len() > 256 {
        return Err(service_response(ServiceError::Invalid(
            aow_i18n::text("validation.invalid_agent_session_identity").to_owned(),
        )));
    }
    // A locator is only cached by a registered-session scan or a trusted
    // automation run lookup. Once one exists, the exact workspace key is
    // enough to read its snapshot, including an automation-created worktree
    // that is intentionally not registered as a project worktree.
    let requested = Path::new(worktree_path);
    {
        let runtime = state.node.agent_node().await.map_err(service_response)?;
        if let Ok(reference) = state.agents.session_locator(requested, agent, session_id) {
            return Ok(reference);
        }
        let cwd = runtime
            .resolve_directory(requested)
            .await
            .map_err(|error| service_response(ServiceError::Invalid(error.to_string())))?;
        state
            .agents
            .session_locator(&cwd, agent, session_id)
            .map_err(service_response)
    }
}
