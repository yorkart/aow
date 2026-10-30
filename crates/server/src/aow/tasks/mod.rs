mod workspace;

use super::*;
use aow_db::tasks::TaskRecord;
use axum::{
    Router,
    routing::{get, post},
};

fn storage(error: anyhow::Error) -> HttpError {
    HttpError::internal(error.to_string())
}

#[derive(Serialize)]
struct Task {
    workspace_mode: String,
    slug: Option<String>,
    id: String,
    project_id: String,
    parent_id: Option<String>,
    prompt: String,
    agent_id: String,
    workspace: String,
    thread_id: String,
    pinned: bool,
    archived: bool,
}
impl From<TaskRecord> for Task {
    fn from(value: TaskRecord) -> Self {
        Self {
            workspace_mode: value.workspace_mode,
            slug: value.slug,
            id: value.id,
            project_id: value.project_id,
            parent_id: value.parent_id,
            prompt: value.prompt,
            agent_id: value.agent_id,
            workspace: value.workspace,
            thread_id: value.thread_id,
            pinned: value.pinned,
            archived: value.archived,
        }
    }
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/tasks", get(list).post(create))
        .route(
            "/api/aow/tasks/{id}",
            axum::routing::patch(pin).delete(remove),
        )
        .route("/api/aow/tasks/{id}/archive", post(archive))
}

impl AowManager {
    /// Serialize ordinary cleanup with task creation so a newly prepared main session
    /// cannot be removed between session allocation and saving its task ownership.
    pub(crate) async fn task_lifecycle_access(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.task_operation.lock().await
    }
    pub(crate) fn protect_task_session(&self, thread: &str) -> Result<(), HttpError> {
        if self
            .inner
            .tasks
            .list()
            .map_err(storage)?
            .iter()
            .any(|task| !task.archived && task.thread_id == thread)
        {
            return Err(crate::instances::invalid(
                "Archive or delete the owning task to clean up its main session",
            ));
        }
        Ok(())
    }
    pub(crate) fn protect_task_workspace(&self, workspace: &str) -> Result<(), AowError> {
        if self
            .inner
            .tasks
            .list()
            .map_err(AowError::Configuration)?
            .iter()
            .any(|task| !task.archived && task.workspace == workspace)
        {
            return Err(AowError::Invalid(
                "Archive or delete tasks using this workspace first".into(),
            ));
        }
        Ok(())
    }
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
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceMode {
    #[default]
    Existing,
    NewWorktree,
}
impl WorkspaceMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::NewWorktree => "new_worktree",
        }
    }
}
#[derive(Deserialize)]
struct Create {
    #[serde(default)]
    workspace_mode: WorkspaceMode,
    slug: Option<String>,
    request_id: Option<String>,
    project_id: String,
    parent_id: Option<String>,
    prompt: String,
    agent_id: String,
    #[serde(default)]
    workspace: String,
}
async fn create(
    State(state): State<AppState>,
    Json(request): Json<Create>,
) -> Result<Json<Task>, HttpError> {
    if request.request_id.as_ref().is_some_and(|id| {
        id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return Err(crate::instances::invalid("Invalid task request ID"));
    }
    if request.prompt.trim().is_empty() || request.prompt.len() > 131_072 {
        return Err(crate::instances::invalid(
            "Prompt must contain 1-131072 bytes",
        ));
    }
    aow_i18n::spawn(async move {
        let _operation = state.aow.inner.task_operation.lock().await;
        let slug = match request.workspace_mode {
            WorkspaceMode::NewWorktree => {
                workspace::task_slug(&request.prompt, request.slug.as_deref())
            }
            WorkspaceMode::Existing => workspace::normalize_slug(request.slug.as_deref()),
        };
        let existing_tasks = state.aow.inner.tasks.list().map_err(storage)?;
        if let Some(existing) = existing_tasks
            .iter()
            .find(|task| request.request_id.as_ref() == Some(&task.id))
        {
            if existing.project_id != request.project_id
                || existing.parent_id != request.parent_id
                || existing.prompt != request.prompt
                || existing.agent_id != request.agent_id
                || existing.workspace_mode != request.workspace_mode.as_str()
                || existing.slug != slug
                || (matches!(request.workspace_mode, WorkspaceMode::Existing)
                    && existing.workspace != request.workspace)
            {
                return Err(crate::instances::invalid(
                    "Task request ID already belongs to a different request",
                ));
            }
            return Ok(Json(Task::from(existing.clone())));
        }
        let _filesystem = state.aow.filesystem_access().await;
        let runtime_parent = request
            .parent_id
            .as_ref()
            .map(|id| {
                existing_tasks
                    .iter()
                    .find(|task| {
                        &task.id == id && task.project_id == request.project_id && !task.archived
                    })
                    .map(|task| aow_protocol::InstanceRef::acp(&task.thread_id))
                    .ok_or_else(|| {
                        crate::instances::invalid(
                            "Parent task must be active and in the same project",
                        )
                    })
            })
            .transpose()?;
        let agent = state
            .acp
            .select_agent_id(Some(&request.agent_id))
            .await
            .map_err(crate::acp::task_error)?;
        let task_id = request.request_id.unwrap_or_else(aow_id::new_id);
        let workspace = match request.workspace_mode {
            WorkspaceMode::Existing => state
                .aow
                .agent_worktree_path(&request.project_id, &request.workspace)
                .await
                .map_err(aow_http_error)?,
            WorkspaceMode::NewWorktree => workspace::create_task_workspace(
                &state,
                &request.project_id,
                &task_id,
                slug.as_deref(),
            )
            .await
            .map_err(aow_http_error)?,
        };
        let session = state
            .acp
            .prepare_session_with_options(
                &agent,
                workspace.clone().into(),
                aow_acp::SessionCreation {
                    source: aow_acp::SessionSource::User,
                    parent: runtime_parent,
                },
            )
            .await
            .map_err(crate::acp::task_error)?;
        let task = TaskRecord {
            workspace_mode: request.workspace_mode.as_str().into(),
            slug,
            id: task_id,
            project_id: request.project_id,
            parent_id: request.parent_id,
            prompt: request.prompt,
            agent_id: agent,
            workspace,
            thread_id: session.thread_id.clone(),
            pinned: false,
            archived: false,
        };
        if let Err(error) = state.aow.inner.tasks.insert(&task) {
            let _ = state.acp.delete_session(&session.thread_id).await;
            return Err(storage(error));
        }
        // The saved identity survives slow startup and client disconnects. ACP records startup errors.
        let _startup = crate::acp::start_task(
            state.clone(),
            task.thread_id.clone(),
            Some(task.prompt.clone()),
        )?;
        Ok(Json(Task::from(task)))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}
#[derive(Deserialize)]
struct Pin {
    pinned: bool,
}
async fn pin(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<Pin>,
) -> Result<StatusCode, HttpError> {
    let _operation = state.aow.inner.task_operation.lock().await;
    state
        .aow
        .inner
        .tasks
        .pin(&id, request.pinned)
        .map_err(storage)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Default, Deserialize)]
struct Finish {
    #[serde(default)]
    controls: HashMap<String, String>,
}
async fn finish(
    state: AppState,
    id: String,
    archive: bool,
    request: Finish,
) -> Result<StatusCode, HttpError> {
    aow_i18n::spawn(async move {
        let _operation = state.aow.inner.task_operation.lock().await;
        let tasks = state.aow.inner.tasks.list().map_err(storage)?;
        if !tasks.iter().any(|task| task.id == id) {
            return Err(crate::instances::invalid("Task not found"));
        }
        let mut ids = std::collections::HashSet::from([id.clone()]);
        loop {
            let before = ids.len();
            for task in &tasks {
                if task
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| ids.contains(parent))
                {
                    ids.insert(task.id.clone());
                }
            }
            if ids.len() == before {
                break;
            }
        }
        for task in tasks
            .iter()
            .filter(|task| ids.contains(&task.id) && !task.archived)
        {
            if state.acp.session_info(&task.thread_id).is_ok() {
                let mut headers = axum::http::HeaderMap::new();
                if let Some(token) = request.controls.get(&task.thread_id) {
                    headers.insert(
                        "x-aow-acp-control",
                        token
                            .parse()
                            .map_err(|_| crate::instances::invalid("Invalid control token"))?,
                    );
                }
                crate::acp::delete_task_session(&state, &task.thread_id, &headers).await?;
            }
        }
        state
            .aow
            .inner
            .tasks
            .finish(&id, archive)
            .map_err(storage)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}
async fn archive(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<Finish>,
) -> Result<StatusCode, HttpError> {
    finish(state, id, true, request).await
}
async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<Finish>,
) -> Result<StatusCode, HttpError> {
    finish(state, id, false, request).await
}

#[cfg(test)]
mod tests;
