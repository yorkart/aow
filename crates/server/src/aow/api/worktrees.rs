use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/projects/{id}/branches", get(project_branches))
        .route("/api/aow/projects/{id}/worktrees", post(create_worktree))
        .route(
            "/api/aow/projects/{id}/worktrees/color",
            post(set_worktree_color),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/icon",
            post(set_worktree_icon),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/removal",
            get(inspect_worktree_removal).delete(remove_worktree),
        )
        .route(
            "/api/aow/projects/{id}/worktrees/removals",
            post(removal::submit_batch),
        )
        .route("/api/aow/worktree-removals", get(removal::list_jobs))
}
async fn project_branches(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<ProjectBranches>, Response> {
    state
        .aow
        .project_branches(&id)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn create_worktree(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateWorktreeRequest>,
) -> Result<impl IntoResponse, Response> {
    let result = state
        .aow
        .create_worktree(&id, request)
        .await
        .map_err(aow_response)?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn set_worktree_color(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SetWorktreeColorRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .set_worktree_color(&id, request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn set_worktree_icon(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SetWorktreeIconRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .set_worktree_icon(&id, request)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn inspect_worktree_removal(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorktreeRemovalQuery>,
) -> Result<Json<WorktreeRemovalPreview>, Response> {
    let inspection = state
        .aow
        .inspect_worktree_removal(&id, &query.path)
        .await
        .map_err(aow_response)?;
    let (terminal_tabs, agent_tabs) = state
        .terminals
        .workspace_tab_counts(&inspection.worktree.path)
        .map_err(|error| crate::terminal::terminal_http_error(error).into_response())?;
    Ok(Json(WorktreeRemovalPreview {
        dirty: !inspection.changes.is_empty(),
        worktree: inspection.worktree,
        changes: inspection.changes,
        change_count: inspection.change_count,
        truncated: inspection.truncated,
        terminal_tabs,
        agent_tabs,
    }))
}

async fn remove_worktree(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorktreeRemovalQuery>,
) -> Result<(StatusCode, Json<removal::RemovalJob>), Response> {
    removal::submit(
        state,
        id,
        vec![removal::RemovalRequest {
            path: query.path,
            force: query.force,
        }],
    )
    .map(|job| (StatusCode::ACCEPTED, Json(job)))
    .map_err(aow_response)
}
