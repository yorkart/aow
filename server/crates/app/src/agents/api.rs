#![expect(
    clippy::result_large_err,
    reason = "HTTP route errors return Axum responses directly without an extra heap allocation"
)]

use super::{AgentsQuery, RegisterAgentRequest, ReorderAgentsRequest, sessions};
use crate::error::{ServiceError, service_response};
use crate::{AppState, HttpError};
use aow_host_contract::agents::AgentRegistration;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Deserialize;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/agents",
            get(list_agents).post(register_agent).patch(reorder_agents),
        )
        .route(
            "/api/agents/settings/default",
            axum::routing::put(set_default_agent),
        )
        .route("/api/agents/{id}", delete(remove_agent).put(update_agent))
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultAgentRequest {
    agent_id: Option<String>,
}

async fn set_default_agent(
    State(state): State<AppState>,
    Json(request): Json<DefaultAgentRequest>,
) -> Result<StatusCode, Response> {
    state
        .agents
        .set_default_agent(request.agent_id)
        .await
        .map_err(service_response)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_agents(
    State(state): State<AppState>,
    Query(_query): Query<AgentsQuery>,
) -> Result<Json<Vec<AgentRegistration>>, Response> {
    state
        .agents
        .agents()
        .await
        .map(Json)
        .map_err(service_response)
}

async fn reorder_agents(
    State(state): State<AppState>,
    Json(request): Json<ReorderAgentsRequest>,
) -> Result<StatusCode, Response> {
    state
        .agents
        .reorder_agents(request)
        .await
        .map_err(service_response)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn register_agent(
    State(state): State<AppState>,
    Json(request): Json<RegisterAgentRequest>,
) -> Result<impl IntoResponse, Response> {
    let agent = state
        .agents
        .register_agent(request)
        .await
        .map_err(service_response)?;
    Ok((StatusCode::CREATED, Json(agent)))
}

async fn remove_agent(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, Response> {
    state
        .agents
        .remove_agent(&id)
        .await
        .map_err(service_response)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_agent(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<RegisterAgentRequest>,
) -> Result<Json<AgentRegistration>, Response> {
    state
        .agents
        .save_agent(Some(&id), request)
        .await
        .map(Json)
        .map_err(service_response)
}

#[derive(Debug, Deserialize)]
struct AgentSessionsQuery {
    worktree_path: String,
    agent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentSessionSnapshotQuery {
    agent: String,
    worktree_path: String,
}

#[derive(Debug, Deserialize)]
struct AutomationRunSessionQuery {
    task_id: String,
    run_id: String,
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
        sessions::public_native_sessions(sessions)
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
        sessions::public_native_sessions(session)
            .map(Json)
            .map_err(|error| HttpError::internal(error.to_string()).into_response())
    }
}

async fn agent_session_snapshot(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AgentSessionSnapshotQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let locator = state
        .agents
        .resolve_session_locator(&session_id, &query.agent, &query.worktree_path)
        .await
        .map_err(service_response)?;
    crate::session_shares::read_snapshot(
        Some(&state.node.agent_node().await.map_err(service_response)?),
        locator,
    )
    .await
    .map(Json)
    .map_err(IntoResponse::into_response)
}
