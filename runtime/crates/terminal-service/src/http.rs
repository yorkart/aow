//! HTTP routes for terminal runtime management.

use crate::{error::ServiceError, websocket::runtime_socket};
use aow_protocol::{
    TerminalAgentList, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec, TerminaldHealth,
};
use aow_runtime_contract::agents::{TerminalAgentProgress, TerminalAgentRequest};
use aow_terminal_core::{AttachOptions, CreateOutcome, TerminalManager};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

const SOCKET_WRITE_BUFFER_SIZE: usize = 64 * 1024;
// An 8 MiB ANSI snapshot can expand up to 6x when JSON escapes control bytes.
const SOCKET_MAX_WRITE_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// Routes relative to the embedding daemon's `/v1/terminal` mount.
pub fn router(state: TerminalManager) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/runtimes", get(list_runtimes))
        .route("/agents", get(list_agents))
        .route(
            "/runtimes/{id}",
            get(get_runtime).put(create_runtime).delete(delete_runtime),
        )
        .route("/runtimes/{id}/attach", get(attach_runtime))
        .route("/runtimes/{id}/screen", get(screen))
        .route(
            "/runtimes/{id}/agent-operation",
            axum::routing::post(admit_agent_operation),
        )
        .route(
            "/runtimes/{id}/agent-operation/{request_id}",
            get(agent_operation_status),
        )
        .with_state(state)
}

async fn screen(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Option<aow_protocol::TerminalScreen>>, ServiceError> {
    Ok(Json(state.screen(&id)?))
}

async fn health(State(state): State<TerminalManager>) -> Json<TerminaldHealth> {
    Json(state.health())
}

async fn list_runtimes(
    State(state): State<TerminalManager>,
) -> Result<Json<TerminalRuntimeList>, ServiceError> {
    Ok(Json(state.list()?))
}

async fn list_agents(
    State(state): State<TerminalManager>,
) -> Result<Json<TerminalAgentList>, ServiceError> {
    Ok(Json(state.agents().await?))
}

async fn get_runtime(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<TerminalRuntime>, ServiceError> {
    Ok(Json(state.get(&id)?))
}

async fn create_runtime(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
    Json(spec): Json<TerminalRuntimeSpec>,
) -> Result<Response, ServiceError> {
    let response = match state.create(id, spec).await? {
        CreateOutcome::Created(runtime) => (StatusCode::CREATED, Json(runtime)).into_response(),
        CreateOutcome::Existing(runtime) => (StatusCode::OK, Json(runtime)).into_response(),
    };
    Ok(response)
}

async fn delete_runtime(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ServiceError> {
    state.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach_runtime(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<AttachQuery>,
    websocket: WebSocketUpgrade,
) -> Result<Response, ServiceError> {
    let connection = state.attach(
        &id,
        AttachOptions {
            epoch: query.epoch,
            after: query.after,
            vt_snapshot: query.capabilities.as_deref() == Some("vt-snapshot-v1"),
            gzip_snapshot: query.snapshot_compression.as_deref() == Some("gzip"),
            controlled: query.control.as_deref() == Some("v2"),
            observe: query.observer.as_deref() == Some("v1"),
        },
    )?;
    Ok(aow_i18n::http::upgrade(
        websocket
            .write_buffer_size(SOCKET_WRITE_BUFFER_SIZE)
            .max_write_buffer_size(SOCKET_MAX_WRITE_BUFFER_SIZE),
        move |socket| runtime_socket(socket, connection),
    ))
}

#[derive(Debug, Default, Deserialize)]
struct AttachQuery {
    epoch: Option<String>,
    after: Option<u64>,
    control: Option<String>,
    capabilities: Option<String>,
    snapshot_compression: Option<String>,
    observer: Option<String>,
}

async fn admit_agent_operation(
    State(state): State<TerminalManager>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<TerminalAgentRequest>,
) -> Result<Json<TerminalAgentProgress>, ServiceError> {
    Ok(Json(state.admit_agent_operation(&id, request).await?))
}

async fn agent_operation_status(
    State(state): State<TerminalManager>,
    AxumPath((id, request_id)): AxumPath<(String, String)>,
) -> Result<Json<TerminalAgentProgress>, ServiceError> {
    Ok(Json(state.agent_operation_status(&id, &request_id)?))
}
