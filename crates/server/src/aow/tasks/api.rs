use super::model::{Resource, Task, storage, validate_prompt};
use super::*;
use aow_db::tasks::{TaskRecord, TaskStatus};
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
            get(super::routing::get).patch(update).delete(remove),
        )
        .route(
            "/api/aow/projects/{id}/router",
            post(super::routing::router),
        )
        .route("/api/aow/tasks/{id}/resources", post(attach))
        .route("/api/aow/tasks/{id}/status", post(super::messages::status))
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
pub(super) enum WorkspaceMode {
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
    pub is_router: bool,
    pub source_thread_id: Option<String>,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
}

pub(super) async fn create(
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
            if task.is_router && request.is_router && task.project_id == request.project_id {
                return Ok(Json(task.clone().into()));
            }
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
                || task.is_router != request.is_router
                || task.source_thread_id != request.source_thread_id
                || task.workspace_intent != request.workspace_intent
                || task.target_task_id != request.target_task_id
            {
                return Err(crate::instances::invalid(
                    "Task request ID belongs to another request",
                ));
            }
            return Ok(Json(task.clone().into()));
        }
        // Reject unresolved references before provisioning a workspace or session.
        for id in super::mentions::role_mentions(&request.prompt) {
            state.aow.agent_role(&id).map_err(aow_http_error)?;
        }
        let source = request
            .source_thread_id
            .as_deref()
            .map(|id| super::routing::source_task(&state, id))
            .transpose()?;
        let message_source = source
            .as_ref()
            .zip(request.source_thread_id.as_deref())
            .map(|(task, thread)| crate::collaboration::source(&state, task, thread))
            .transpose()
            .map_err(storage)?;
        if source.is_some()
            && !matches!(
                request.workspace_intent.as_deref(),
                Some("change" | "analyze" | "review")
            )
        {
            return Err(crate::instances::invalid(
                "Delegation requires a workspace intent",
            ));
        }
        if source.is_none()
            && (request.workspace_intent.is_some() || request.target_task_id.is_some())
        {
            return Err(crate::instances::invalid(
                "Delegation requires a source conversation",
            ));
        }
        if request.target_task_id.is_some() && request.workspace_intent.as_deref() != Some("review")
        {
            return Err(crate::instances::invalid(
                "Only reviews accept a target task",
            ));
        }
        if matches!(
            request.workspace_intent.as_deref(),
            Some("analyze" | "review")
        ) && !matches!(request.workspace_mode, WorkspaceMode::Existing)
        {
            return Err(crate::instances::invalid(
                "Analysis and review use an existing workspace",
            ));
        }
        if source
            .as_ref()
            .is_some_and(|task| task.project_id != request.project_id)
        {
            return Err(crate::instances::invalid(
                "Delegation must stay within the source project",
            ));
        }
        if request.is_router
            && (source.is_some()
                || request.workspace_intent.is_some()
                || request.target_task_id.is_some()
                || !request.dependencies.is_empty()
                || !matches!(request.workspace_mode, WorkspaceMode::Existing))
        {
            return Err(crate::instances::invalid(
                "Router must use the project workspace without a delegation source",
            ));
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
        if role
            .as_ref()
            .is_some_and(|role| matches!(role.config.runtime, roles::RoleRuntime::Cli { .. }))
        {
            return Err(crate::instances::invalid(
                "Task collaboration requires an ACP role",
            ));
        }
        let agent = if let Some(role) = &role {
            state.aow.check_role_runtime(role, &state).await?;
            match &role.config.runtime {
                roles::RoleRuntime::Acp { agent_id } => agent_id.clone(),
                roles::RoleRuntime::Cli { .. } => format!("role:{}", role.id),
            }
        } else {
            let agent = state
                .acp
                .select_agent_id(
                    (!request.agent_id.is_empty()).then_some(request.agent_id.as_str()),
                )
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
        let target_revision =
            super::routing::validate_workspace(&state, &request, &workspace).await?;
        // Dependencies are organizational, never runtime parents.
        let session = if let Some(role) = &role {
            state
                .aow
                .prepare_role_session(
                    role,
                    &state,
                    &workspace,
                    Some(&request.prompt),
                    request
                        .source_thread_id
                        .as_ref()
                        .map(aow_protocol::InstanceRef::acp),
                )
                .await?
        } else {
            roles::RoleSession::Acp {
                session: Box::new(
                    state
                        .acp
                        .prepare_session_with_options(
                            &agent,
                            workspace.clone().into(),
                            aow_acp::SessionCreation {
                                parent: request
                                    .source_thread_id
                                    .as_ref()
                                    .map(aow_protocol::InstanceRef::acp),
                                ..Default::default()
                            },
                        )
                        .await
                        .map_err(crate::acp::task_error)?,
                ),
            }
        };
        let (kind, resource_id) = session.resource();
        let mut task = TaskRecord {
            id: request.request_id,
            project_id: request.project_id,
            status: TaskStatus::Todo,
            prompt: request.prompt,
            agent_id: agent,
            agent_role_id: request.agent_role_id,
            is_router: request.is_router,
            source_task_id: source.map(|task| task.id),
            source_thread_id: request.source_thread_id,
            workspace_intent: request.workspace_intent,
            target_task_id: request.target_task_id,
            target_revision,
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
        let message = (!task.is_router && task.initial_thread_id.is_some()).then(|| {
            crate::collaboration::new_message(
                &task,
                "initial".into(),
                task.prompt.clone(),
                message_source,
            )
        });
        if message.is_some() {
            task.status = TaskStatus::InProgress;
        }
        if let Err(error) = state
            .aow
            .inner
            .tasks
            .insert_with_message(&task, message.as_ref())
        {
            session.discard(&state).await;
            return Err(crate::instances::invalid(error.to_string()));
        }
        if message.is_some() {
            state.collaboration.start(&state);
        } else {
            session.start(&state, (!task.is_router).then(|| task.prompt.clone()))?;
        }
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
    if state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .iter()
        .any(|task| task.id == id && task.is_router)
        && (request.archived == Some(true) || request.dependencies.is_some())
    {
        return Err(crate::instances::invalid(
            "The project Router remains active and has no task dependencies",
        ));
    }
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
    if state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .iter()
        .any(|task| task.id == id && task.is_router)
    {
        return Err(crate::instances::invalid(
            "The project Router cannot be deleted as a task",
        ));
    }
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
