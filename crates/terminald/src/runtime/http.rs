//! HTTP routes for terminal runtime management.

use super::websocket::runtime_socket;
use super::*;

pub(crate) fn build_router() -> Router {
    router_with_state(DaemonState::new())
}

pub(super) fn router_with_state(state: DaemonState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/runtimes", get(list_runtimes))
        .route("/v1/agents", get(list_agents))
        .route(
            "/v1/runtimes/{id}",
            get(get_runtime).put(create_runtime).delete(delete_runtime),
        )
        .route("/v1/runtimes/{id}/attach", get(attach_runtime))
        .route("/v1/runtimes/{id}/screen", get(screen))
        .with_state(state)
}

async fn screen(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Option<aow_protocol::TerminalScreen>>, TerminaldError> {
    state.screen(&id).map(Json)
}

async fn health(State(state): State<DaemonState>) -> Json<TerminaldHealth> {
    Json(state.health())
}

async fn list_runtimes(
    State(state): State<DaemonState>,
) -> Result<Json<TerminalRuntimeList>, TerminaldError> {
    Ok(Json(state.list()?))
}

async fn list_agents(
    State(state): State<DaemonState>,
) -> Result<Json<TerminalAgentList>, TerminaldError> {
    tokio::task::spawn_blocking(move || state.agents())
        .await
        .map_err(|error| TerminaldError::Worker(error.to_string()))?
        .map(Json)
}

async fn get_runtime(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<TerminalRuntime>, TerminaldError> {
    Ok(Json(state.get(&id)?))
}

async fn create_runtime(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
    Json(spec): Json<TerminalRuntimeSpec>,
) -> Result<Response, TerminaldError> {
    let response = match state.create(id, spec).await? {
        CreateOutcome::Created(runtime) => (StatusCode::CREATED, Json(runtime)).into_response(),
        CreateOutcome::Existing(runtime) => (StatusCode::OK, Json(runtime)).into_response(),
    };
    Ok(response)
}

async fn delete_runtime(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, TerminaldError> {
    state.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach_runtime(
    State(state): State<DaemonState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<AttachQuery>,
    websocket: WebSocketUpgrade,
) -> Result<Response, TerminaldError> {
    let connection = state.attach(
        &id,
        query.epoch.as_deref(),
        query.after,
        query.capabilities.as_deref() == Some("vt-snapshot-v1"),
    )?;
    let controlled = query.control.as_deref() == Some("v2");
    let observe = query.observer.as_deref() == Some("v1");
    Ok(websocket
        .write_buffer_size(SOCKET_WRITE_BUFFER_SIZE)
        .max_write_buffer_size(SOCKET_MAX_WRITE_BUFFER_SIZE)
        .on_upgrade(move |socket| runtime_socket(socket, connection, controlled, observe))
        .into_response())
}

#[derive(Debug, Default, Deserialize)]
struct AttachQuery {
    epoch: Option<String>,
    after: Option<u64>,
    control: Option<String>,
    capabilities: Option<String>,
    observer: Option<String>,
}
