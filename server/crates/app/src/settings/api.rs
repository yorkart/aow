#![expect(
    clippy::result_large_err,
    reason = "HTTP route errors return Axum responses directly without an extra heap allocation"
)]

use super::*;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .merge(super::server_environment_routes())
        .route("/api/aow/settings", get(get_settings).put(update_settings))
        .route(
            "/api/aow/settings/discovered-path",
            get(get_discovered_path),
        )
        .route(
            "/api/aow/pinned-worktrees",
            get(get_pinned_worktrees).patch(update_pinned_worktrees),
        )
        .route(
            "/api/aow/pinned-directories",
            get(get_pinned_directories).patch(update_pinned_directories),
        )
}
async fn get_settings(State(state): State<AppState>) -> Result<Json<AowSettings>, Response> {
    let paths = state
        .settings
        .execution_path()
        .await
        .map_err(service_response)?;
    let mut settings = state.settings.settings().map_err(service_response)?;
    settings.execution_path = Some(paths);
    Ok(Json(settings))
}

async fn get_discovered_path(
    State(state): State<AppState>,
) -> Result<Json<Vec<PathBuf>>, Response> {
    state
        .node
        .agent_node()
        .await
        .map_err(service_response)?
        .discovered_execution_path()
        .await
        .map(Json)
        .map_err(|error| service_response(ServiceError::Invalid(error.to_string())))
}

async fn get_pinned_worktrees(
    State(state): State<AppState>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state
        .settings
        .pinned_worktrees()
        .map(Json)
        .map_err(service_response)
}

async fn update_pinned_worktrees(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedWorktreesRequest>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state
        .settings
        .update_pinned_worktrees(request)
        .map(Json)
        .map_err(service_response)
}

async fn get_pinned_directories(
    State(state): State<AppState>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .settings
        .pinned_directories()
        .map(Json)
        .map_err(service_response)
}

async fn update_pinned_directories(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedDirectoriesRequest>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .settings
        .update_pinned_directories(request)
        .await
        .map(Json)
        .map_err(service_response)
}

async fn update_settings(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<AowSettings>, Response> {
    state
        .settings
        .update_settings(request)
        .await
        .map(Json)
        .map_err(service_response)
}
