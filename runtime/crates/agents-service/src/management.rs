//! HTTP adapters for native Agent management.
use aow_agents_core::management::{self, AgentManager};
use aow_runtime_contract::agents::*;
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use std::{collections::BTreeMap, path::PathBuf};

type Response<T> = Result<Json<T>, (StatusCode, Json<serde_json::Value>)>;
fn error(error: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"code":"node_agent_error","message":error.to_string()})),
    )
}
pub fn router(root: PathBuf) -> Router {
    Router::new()
        .route(
            "/v1/environment/discovered-path",
            axum::routing::get(discovered_path),
        )
        .route(
            "/v1/agents/configuration-environment",
            axum::routing::get(configuration_environment),
        )
        .route("/v1/agents/capabilities", axum::routing::get(capabilities))
        .route("/v1/agents/validate-automation", post(validate_automation))
        .route("/v1/agents/match-terminal", post(match_terminal))
        .route(
            "/v1/agents/prepare-role-session",
            post(prepare_role_session),
        )
        .route("/v1/agents/probe", post(probe))
        .route("/v1/agents/role-layout", post(role_layout))
        .route("/v1/agents/discover", post(discover))
        .route("/v1/agents/resolve", post(resolve))
        .route("/v1/agents/prepare-role", post(prepare_role))
        .route("/v1/agents/prepare-terminal", post(prepare))
        .with_state(AgentManager::new(root))
}

async fn discover(
    State(node): State<AgentManager>,
    Json(request): Json<DiscoverAgents>,
) -> Response<Vec<AgentRegistration>> {
    management::discover(&node, request)
        .await
        .map(Json)
        .map_err(error)
}

async fn resolve(
    State(node): State<AgentManager>,
    Json(request): Json<ResolveAgent>,
) -> Response<AgentLaunch> {
    management::resolve(&node, request)
        .await
        .map(Json)
        .map_err(error)
}

async fn prepare(
    State(node): State<AgentManager>,
    Json(request): Json<PrepareTerminal>,
) -> Response<AgentLaunch> {
    management::prepare(&node, request)
        .await
        .map(Json)
        .map_err(error)
}

async fn prepare_role(Json(request): Json<PrepareRole>) -> Response<AgentLaunch> {
    management::prepare_role(request)
        .await
        .map(Json)
        .map_err(error)
}

async fn validate_automation(Json(request): Json<ValidateAutomation>) -> Response<()> {
    management::validate_automation(request)
        .await
        .map(Json)
        .map_err(error)
}

async fn prepare_role_session(
    Json(request): Json<PrepareRoleSession>,
) -> Response<RoleSessionPlan> {
    management::prepare_role_session(request)
        .await
        .map(Json)
        .map_err(error)
}

async fn role_layout(
    Json(request): Json<ResolveRoleLayout>,
) -> Response<aow_runtime_environment::HomeLayout> {
    management::role_layout(request)
        .await
        .map(Json)
        .map_err(error)
}

async fn probe(State(node): State<AgentManager>, Json(request): Json<ProbeAgent>) -> Response<()> {
    management::probe::run(&node, request)
        .await
        .map(Json)
        .map_err(error)
}

async fn discovered_path() -> Json<Vec<PathBuf>> {
    Json(management::discovered_path().await)
}

async fn capabilities() -> Json<Vec<AgentCapabilities>> {
    Json(management::capabilities().await)
}

async fn configuration_environment() -> Json<BTreeMap<String, String>> {
    Json(management::configuration_environment().await)
}

async fn match_terminal(Json(request): Json<MatchTerminalProfile>) -> Json<Vec<String>> {
    Json(management::match_terminal(request).await)
}
