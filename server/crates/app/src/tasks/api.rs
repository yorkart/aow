use super::model::{Resource, Task, storage, validate_prompt};
use super::*;
use axum::{
    Router,
    http::HeaderMap,
    routing::{get, post},
};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list).post(create))
        .route(
            "/api/tasks/{id}",
            get(super::routing::get).patch(update).delete(remove),
        )
        .route(
            "/api/projects/{id}/orchestrator",
            post(super::routing::orchestrator),
        )
        .route("/api/tasks/{id}/resources", post(attach))
        .route("/api/tasks/{id}/status", post(super::messages::status))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<Task>>, HttpError> {
    Ok(Json(
        state
            .tasks
            .inner
            .tasks
            .list()
            .map_err(storage)?
            .into_iter()
            .map(Task::from)
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WorkspaceMode {
    Existing,
    NewWorktree,
}

impl WorkspaceMode {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::NewWorktree => "new_worktree",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    pub request_id: String,
    pub project_id: String,
    pub prompt: String,
    #[serde(default)]
    pub agent_id: String,
    pub agent_role_id: Option<String>,
    pub workspace_mode: WorkspaceMode,
    pub workspace: Option<String>,
    pub slug: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub is_orchestrator: bool,
    pub source_thread_id: Option<String>,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
}

pub(super) async fn create(
    State(state): State<AppState>,
    Json(request): Json<Create>,
) -> Result<Json<Task>, HttpError> {
    super::workflow::create(state, request).await.map(Json)
}

#[derive(Deserialize)]
struct Update {
    prompt: Option<String>,
    pinned: Option<bool>,
    archived: Option<bool>,
    dependencies: Option<Vec<String>>,
}

async fn update(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<Update>,
) -> Result<StatusCode, HttpError> {
    if let Some(prompt) = &request.prompt {
        validate_prompt(prompt)?;
    }
    let _operation = state.tasks.task_resource_access().await;
    let task = super::routing::find_task(&state, &id)?;
    if task.is_orchestrator && request.dependencies.is_some() {
        return Err(crate::instances::invalid(
            "The project Orchestrator has no task dependencies",
        ));
    }
    if task.archived && request.archived == Some(false) {
        let thread = task
            .initial_thread_id
            .as_deref()
            .ok_or_else(|| crate::instances::invalid("Task has no main conversation"))?;
        state.acp.session_info(thread).await.map_err(|error| {
            crate::instances::invalid(
                format!("Cannot restore this task while its original main conversation is unavailable: {error}"),
            )
        })?;
    }
    state
        .tasks
        .inner
        .tasks
        .update(
            &id,
            request.prompt.as_deref(),
            request.pinned,
            request.archived,
            request.dependencies.as_deref(),
        )
        .map_err(|error| crate::instances::invalid(error.to_string()))?;
    state.workspace_events.projects_changed();
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, HttpError> {
    let _operation = state.tasks.task_resource_access().await;
    state
        .tasks
        .inner
        .tasks
        .remove(&id)
        .map_err(|error| crate::instances::invalid(error.to_string()))?;
    state.workspace_events.projects_changed();
    Ok(StatusCode::NO_CONTENT)
}

async fn attach(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(resource): Json<Resource>,
) -> Result<StatusCode, HttpError> {
    let _operation = state.tasks.task_resource_access().await;
    let workspace = match resource.kind.as_str() {
        "acp" => {
            state
                .acp
                .session_info(&resource.id)
                .await
                .map_err(crate::acp::task_error)?
                .cwd
        }
        "terminal" => {
            state
                .terminals
                .get(&resource.id)
                .await
                .map_err(crate::terminal::terminal_http_error)?
                .workspace_root
        }
        _ => return Err(crate::instances::invalid("Unsupported task resource")),
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-aow-task",
        id.parse()
            .map_err(|_| crate::instances::invalid("Invalid task ID"))?,
    );
    state
        .tasks
        .task_scope(&headers, Path::new(&workspace))
        .await?;
    state
        .tasks
        .attach_task_resource(Some(&id), &resource.kind, &resource.id)?;
    Ok(StatusCode::NO_CONTENT)
}
