use super::model::{Resource, Task, storage, validate_prompt};
use super::*;
use aow_db::tasks::TaskRecord;
use axum::{
    Router,
    http::HeaderMap,
    routing::{get, post},
};

pub(in crate::aow) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/tasks", get(list).post(create))
        .route(
            "/api/aow/tasks/{id}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/api/aow/tasks/{id}/resources", post(attach))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<Task>>, HttpError> {
    Ok(Json(
        state
            .aow
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
enum WorkspaceMode {
    Existing,
    NewWorktree,
}

impl WorkspaceMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::NewWorktree => "new_worktree",
        }
    }
}

#[derive(Deserialize)]
struct Create {
    request_id: String,
    project_id: String,
    prompt: String,
    #[serde(default)]
    agent_id: String,
    agent_role_id: Option<String>,
    workspace_mode: WorkspaceMode,
    workspace: Option<String>,
    slug: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(request): Json<Create>,
) -> Result<Json<Task>, HttpError> {
    validate_prompt(&request.prompt)?;
    if request.agent_role_id.is_some() && !request.agent_id.is_empty() {
        return Err(crate::instances::invalid(
            "Choose either an ACP agent or an agent role",
        ));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > 128
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(crate::instances::invalid("Invalid task request ID"));
    }
    match request.workspace_mode {
        WorkspaceMode::Existing
            if request
                .workspace
                .as_deref()
                .is_none_or(|path| path.trim().is_empty()) =>
        {
            return Err(crate::instances::invalid(
                "An existing workspace is required",
            ));
        }
        WorkspaceMode::NewWorktree if request.workspace.is_some() => {
            return Err(crate::instances::invalid(
                "New task worktrees use the project defaults",
            ));
        }
        _ => {}
    }
    let slug = workspace::task_slug(&request.prompt, request.slug.as_deref());
    // Save identity and start execution even if the requesting browser disconnects.
    aow_i18n::spawn(async move {
        let _operation = state.aow.task_resource_access().await;
        let tasks = state.aow.inner.tasks.list().map_err(storage)?;
        if let Some(task) = tasks.iter().find(|task| task.id == request.request_id) {
            if task.project_id != request.project_id
                || task.prompt != request.prompt
                || task.agent_role_id != request.agent_role_id
                || (request.agent_role_id.is_none() && task.agent_id != request.agent_id)
                || task.workspace_mode != request.workspace_mode.as_str()
                || task.slug != slug
                || (matches!(request.workspace_mode, WorkspaceMode::Existing)
                    && request
                        .workspace
                        .as_deref()
                        .and_then(|path| std::fs::canonicalize(path).ok())
                        .as_deref()
                        != Some(Path::new(&task.workspace)))
                || task.dependencies != request.dependencies
            {
                return Err(crate::instances::invalid(
                    "Task request ID belongs to another request",
                ));
            }
            return Ok(Json(task.clone().into()));
        }
        let mut seen = std::collections::HashSet::new();
        for dependency in &request.dependencies {
            if !seen.insert(dependency)
                || !tasks
                    .iter()
                    .any(|task| task.id == *dependency && task.project_id == request.project_id)
            {
                return Err(crate::instances::invalid(
                    "Dependencies must be unique and exist in the same project",
                ));
            }
        }
        let _filesystem = state.aow.filesystem_access().await;
        let role = request
            .agent_role_id
            .as_deref()
            .map(|id| state.aow.agent_role(id))
            .transpose()
            .map_err(aow_http_error)?;
        let agent = if let Some(role) = &role {
            state.aow.check_role_runtime(role, &state).await?;
            match &role.config.runtime {
                roles::RoleRuntime::Acp { agent_id } => agent_id.clone(),
                roles::RoleRuntime::Cli { .. } => format!("role:{}", role.id),
            }
        } else {
            let agent = state
                .acp
                .select_agent_id(Some(&request.agent_id))
                .await
                .map_err(crate::acp::task_error)?;
            if !state
                .acp
                .agents()
                .await
                .map_err(crate::acp::task_error)?
                .iter()
                .any(|item| item.id == agent && item.supported && item.installed)
            {
                return Err(crate::instances::invalid("ACP agent is not available"));
            }
            agent
        };
        let path = match request.workspace_mode {
            WorkspaceMode::Existing => request.workspace.as_deref().unwrap().to_owned(),
            WorkspaceMode::NewWorktree => workspace::create_task_workspace(
                &state,
                &request.project_id,
                &request.request_id,
                slug.as_deref(),
            )
            .await
            .map_err(aow_http_error)?,
        };
        let workspace = state
            .aow
            .agent_worktree_path(&request.project_id, &path)
            .await
            .map_err(aow_http_error)?;
        // Dependencies are organizational, never runtime parents.
        let session = if let Some(role) = &role {
            state
                .aow
                .prepare_role_session(role, &state, &workspace, Some(&request.prompt))
                .await?
        } else {
            roles::RoleSession::Acp {
                session: Box::new(
                    state
                        .acp
                        .prepare_session(&agent, workspace.clone().into())
                        .await
                        .map_err(crate::acp::task_error)?,
                ),
            }
        };
        let (kind, resource_id) = session.resource();
        let task = TaskRecord {
            id: request.request_id,
            project_id: request.project_id,
            prompt: request.prompt,
            agent_id: agent,
            agent_role_id: request.agent_role_id,
            workspace,
            workspace_mode: request.workspace_mode.as_str().into(),
            slug,
            initial_thread_id: (kind == "acp").then(|| resource_id.to_owned()),
            pinned: false,
            archived: false,
            dependencies: request.dependencies,
            resources: vec![TaskResource {
                kind: kind.into(),
                id: resource_id.into(),
            }],
        };
        if let Err(error) = state.aow.inner.tasks.insert(&task) {
            session.discard(&state).await;
            return Err(crate::instances::invalid(error.to_string()));
        }
        session.start(&state, Some(task.prompt.clone()))?;
        Ok(Json(task.into()))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
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
    let _operation = state.aow.task_resource_access().await;
    state
        .aow
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
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, HttpError> {
    let _operation = state.aow.task_resource_access().await;
    state
        .aow
        .inner
        .tasks
        .remove(&id)
        .map_err(|error| crate::instances::invalid(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(resource): Json<Resource>,
) -> Result<StatusCode, HttpError> {
    let _operation = state.aow.task_resource_access().await;
    let workspace = match resource.kind.as_str() {
        "acp" => {
            state
                .acp
                .session_info(&resource.id)
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
    state.aow.task_scope(&headers, Path::new(&workspace))?;
    state
        .aow
        .attach_task_resource(Some(&id), &resource.kind, &resource.id)?;
    Ok(StatusCode::NO_CONTENT)
}
