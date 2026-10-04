use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/projects",
            get(list_projects).post(register_project),
        )
        .route("/api/aow/projects/{id}", delete(remove_project))
        .route("/api/aow/projects/{id}/refresh", post(refresh_project))
        .route(
            "/api/aow/projects/{id}/avatar",
            get(project_avatar::get_avatar),
        )
        .route(
            "/api/aow/projects/{id}/notes/bind",
            post(bind_project_notes),
        )
        .route(
            "/api/aow/projects/{id}/notes/temporary",
            post(create_temporary_project_note),
        )
}
async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, Response> {
    state.aow.projects().await.map(Json).map_err(aow_response)
}

async fn register_project(
    State(state): State<AppState>,
    Json(request): Json<RegisterProjectRequest>,
) -> Result<impl IntoResponse, Response> {
    let project = state
        .aow
        .register_project(request)
        .await
        .map_err(aow_response)?;
    state.workspace_events.projects_changed();
    Ok((StatusCode::CREATED, Json(project)))
}

async fn remove_project(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, Response> {
    state.aow.remove_project(&id).await.map_err(aow_response)?;
    state.workspace_events.projects_changed();
    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_project(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Project>, Response> {
    state.aow.project(&id).await.map(Json).map_err(aow_response)
}

async fn bind_project_notes(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<BindNotesRequest>,
) -> Result<Json<Project>, Response> {
    state
        .aow
        .bind_notes(&id, &request.path)
        .await
        .map(Json)
        .map_err(aow_response)
}

async fn create_temporary_project_note(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateTemporaryNoteRequest>,
) -> Result<impl IntoResponse, Response> {
    let _filesystem = state.aow.filesystem_access().await;
    let root = notes::notes_root_canonical(&state.aow, &id)
        .await
        .map_err(aow_response)?;
    let (path, name) =
        notes::create_temporary_note_file(&root, request.extension.as_str(), aow_id::new_id)
            .await
            .map_err(aow_response)?;
    Ok((
        StatusCode::CREATED,
        Json(TemporaryNoteResult {
            path: path.to_string_lossy().into_owned(),
            name,
            kind: "file",
        }),
    ))
}
