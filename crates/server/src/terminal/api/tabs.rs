use super::*;

pub(super) async fn list_terminals(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TerminalTabList>, HttpError> {
    Ok(Json(
        state
            .terminals
            .list(query.workspace_root.as_deref())
            .await
            .map_err(terminal_http_error)?,
    ))
}

pub(super) async fn list_terminal_agents(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TerminalAgentList>, HttpError> {
    state
        .terminals
        .agents(query.workspace_root.as_deref())
        .await
        .map(Json)
        .map_err(terminal_http_error)
}

pub(super) async fn get_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .get(&tab_id)
            .await
            .map_err(terminal_http_error)?,
    ))
}

pub(super) async fn create_terminal(
    State(state): State<AppState>,
    Json(request): Json<CreateTerminalRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let tab = if let Some(agent_id) = request.agent_id.clone() {
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())
            .map_err(terminal_http_error)?;
        // Native sessions can originate in a worktree subdirectory. Resolve the
        // profile against the registered workspace while retaining that session cwd.
        let launch_workspace = if request.resume_session_id.is_some() {
            request.workspace_root.as_deref().unwrap_or(&cwd)
        } else {
            &cwd
        };
        let launch = state
            .aow
            .resolve_agent_launch(&agent_id, launch_workspace)
            .await
            .map_err(crate::aow::aow_http_error)?;
        state.terminals.create_agent(request, launch).await
    } else {
        state.terminals.create(request).await
    }
    .map_err(terminal_http_error)?;
    Ok((StatusCode::CREATED, Json(tab)))
}

pub(super) async fn update_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<UpdateTerminalRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .update(&tab_id, request)
            .map_err(terminal_http_error)?,
    ))
}

pub(super) async fn reorder_terminals(
    State(state): State<AppState>,
    Json(request): Json<ReorderTerminalsRequest>,
) -> Result<Json<TerminalTabList>, HttpError> {
    Ok(Json(
        state
            .terminals
            .reorder(request)
            .map_err(terminal_http_error)?,
    ))
}

pub(super) async fn delete_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<StatusCode, HttpError> {
    state
        .terminals
        .delete_tab(&tab_id)
        .await
        .map_err(terminal_http_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn split_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<SplitTerminalRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .split(&tab_id, request)
            .await
            .map_err(terminal_http_error)?,
    ))
}

pub(super) async fn delete_terminal_pane(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
) -> Result<Response, HttpError> {
    match state
        .terminals
        .delete_pane(&tab_id, &pane_id)
        .await
        .map_err(terminal_http_error)?
    {
        Some(tab) => Ok(Json(tab).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub(super) async fn update_terminal_layout(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
    Json(request): Json<UpdateLayoutRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    Ok(Json(
        state
            .terminals
            .update_layout(&tab_id, request)
            .map_err(terminal_http_error)?,
    ))
}
