#![expect(
    clippy::result_large_err,
    reason = "HTTP route errors return Axum responses directly without an extra heap allocation"
)]

use super::*;

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
