use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/agents", get(list_agents).post(register_agent))
        .route("/api/aow/agents/{id}", delete(remove_agent))
}
async fn list_agents(
    State(state): State<AppState>,
    Query(_query): Query<AgentsQuery>,
) -> Result<Json<Vec<AgentRegistration>>, Response> {
    state.aow.agents().await.map(Json).map_err(aow_response)
}

async fn register_agent(
    State(state): State<AppState>,
    Json(request): Json<RegisterAgentRequest>,
) -> Result<impl IntoResponse, Response> {
    let agent = state
        .aow
        .register_agent(request)
        .await
        .map_err(aow_response)?;
    Ok((StatusCode::CREATED, Json(agent)))
}

async fn remove_agent(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, Response> {
    state.aow.remove_agent(&id).map_err(aow_response)?;
    Ok(StatusCode::NO_CONTENT)
}
