use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
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
    state.aow.execution_path().await.map_err(aow_response)?;
    state.aow.settings().map(Json).map_err(aow_response)
}

async fn get_discovered_path() -> Json<Vec<PathBuf>> {
    Json(discovery_path().await)
}

async fn get_pinned_worktrees(
    State(state): State<AppState>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state.aow.pinned_worktrees().map(Json).map_err(aow_response)
}

async fn update_pinned_worktrees(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedWorktreesRequest>,
) -> Result<Json<PinnedWorktrees>, Response> {
    state
        .aow
        .update_pinned_worktrees(request)
        .map(Json)
        .map_err(aow_response)
}

async fn get_pinned_directories(
    State(state): State<AppState>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .aow
        .pinned_directories()
        .map(Json)
        .map_err(aow_response)
}

async fn update_pinned_directories(
    State(state): State<AppState>,
    Json(request): Json<UpdatePinnedDirectoriesRequest>,
) -> Result<Json<PinnedDirectories>, Response> {
    state
        .aow
        .update_pinned_directories(request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn update_settings(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<AowSettings>, Response> {
    state
        .aow
        .update_settings(request)
        .await
        .map(Json)
        .map_err(aow_response)
}
