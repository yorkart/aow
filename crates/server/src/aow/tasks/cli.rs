//! Same-user task operations. This router is bound only to the private CLI socket.
use super::*;
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/tasks", get(list_tasks).post(create_task))
        .route("/v1/tasks/{id}", get(get_task))
        .route("/v1/tasks/{id}/submit", post(submit_task))
        .route("/v1/tasks/{id}/archive", post(archive_task))
        .route("/v1/tasks/{id}/delete", post(delete_task))
}
fn missing(message: &str) -> HttpError {
    HttpError::new(StatusCode::NOT_FOUND, "task_not_found", message, None)
}
fn conflict(message: &str) -> HttpError {
    HttpError::new(StatusCode::CONFLICT, "task_conflict", message, None)
}
fn task(state: &AppState, id: &str) -> Result<TaskRecord, HttpError> {
    state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| missing("Task not found"))
}
fn sync_bindings(state: &AppState) -> Result<(), HttpError> {
    for info in state.acp.sessions(None) {
        if let Some(session) = &info.session_id {
            state
                .aow
                .bind_task_session(&info.thread_id, &info.agent_id, session)
                .map_err(storage)?;
        }
    }
    Ok(())
}
fn caller(state: &AppState, session_id: &str) -> Result<TaskRecord, HttpError> {
    if session_id.trim().is_empty() {
        return Err(crate::instances::invalid("session_id must not be empty"));
    }
    sync_bindings(state)?;
    let matches: Vec<_> = state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .filter(|row| !row.archived && row.session_id.as_deref() == Some(session_id))
        .collect();
    match matches.len() {
        0 => Err(missing("No active Task owns this ACP session ID")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(conflict(
            "ACP session ID is ambiguous across runtimes; task context cannot be inferred",
        )),
    }
}
fn detail(state: &AppState, id: &str) -> Result<Value, HttpError> {
    sync_bindings(state)?;
    let task = task(state, id)?;
    let session = if task.archived {
        None
    } else {
        state.acp.overview(&task.thread_id).ok()
    };
    let mut value = serde_json::to_value(Task::from(task))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    value["status"] = json!(
        session
            .as_ref()
            .map(|snapshot| snapshot.status.as_str())
            .unwrap_or(if value["archived"] == true {
                "archived"
            } else {
                "unavailable"
            })
    );
    value["session"] = json!(session);
    Ok(value)
}
#[derive(Deserialize)]
struct CreateTask {
    from_session_id: Option<String>,
    project_id: Option<String>,
    agent_runtime_id: Option<String>,
    request_id: Option<String>,
    task: String,
    #[serde(default = "current_workspace")]
    worktree: String,
    slug: Option<String>,
    #[serde(default = "timeout_seconds")]
    timeout_seconds: u64,
}
fn current_workspace() -> String {
    "current".into()
}
fn timeout_seconds() -> u64 {
    120
}
fn validate_timeout(value: u64) -> Result<(), HttpError> {
    if !(1..=600).contains(&value) {
        return Err(crate::instances::invalid("timeout_seconds must be 1-600"));
    }
    Ok(())
}
async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTask>,
) -> Result<Json<Value>, HttpError> {
    validate_timeout(request.timeout_seconds)?;
    let parent = request
        .from_session_id
        .as_deref()
        .map(|id| caller(&state, id))
        .transpose()?;
    let project_id = parent
        .as_ref()
        .map(|task| task.project_id.clone())
        .or(request.project_id.clone())
        .ok_or_else(|| {
            crate::instances::invalid(
                "Provide from_session_id for a child task or project_id for a root task",
            )
        })?;
    if request
        .project_id
        .as_ref()
        .is_some_and(|project| project != &project_id)
    {
        return Err(crate::instances::invalid(
            "Project does not match the calling Task",
        ));
    }
    let runtime = match request
        .agent_runtime_id
        .or_else(|| parent.as_ref().map(|task| task.agent_runtime_id.clone()))
    {
        Some(runtime) => runtime,
        None => state
            .acp
            .select_agent_id(None)
            .await
            .map_err(crate::acp::task_error)?,
    };
    let workspace = match request.worktree.as_str() {
        "new" => String::new(),
        "current" => match &parent {
            Some(parent) => parent.workspace.clone(),
            None => {
                state
                    .aow
                    .project(&project_id)
                    .await
                    .map_err(aow_http_error)?
                    .worktrees
                    .into_iter()
                    .find(|tree| tree.is_main)
                    .ok_or_else(|| missing("Main checkout not found"))?
                    .path
            }
        },
        path => path.to_owned(),
    };
    let parent_id = parent.map(|parent| parent.id);
    let created = create(
        State(state.clone()),
        Json(Create {
            request_id: request.request_id,
            project_id,
            parent_id,
            prompt: request.task,
            agent_runtime_id: runtime,
            workspace_mode: if request.worktree == "new" {
                WorkspaceMode::NewWorktree
            } else {
                WorkspaceMode::Existing
            },
            workspace,
            slug: request.slug,
        }),
    )
    .await?
    .0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(request.timeout_seconds);
    loop {
        let snapshot = state
            .acp
            .snapshot(&created.thread_id)
            .map_err(crate::acp::task_error)?;
        let phase = if snapshot.error.is_some() {
            Some("failed")
        } else if snapshot.session_id.is_some()
            && snapshot.entries.iter().any(|entry| entry.kind == "user")
        {
            Some("ready")
        } else if tokio::time::Instant::now() >= deadline {
            Some("starting")
        } else {
            None
        };
        if let Some(phase) = phase {
            let mut result = detail(&state, &created.id)?;
            result["phase"] = json!(phase);
            result["startup_error"] = json!(snapshot.error);
            return Ok(Json(result));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[derive(Default, Deserialize)]
struct ListQuery {
    from_session_id: Option<String>,
    project_id: Option<String>,
    parent_id: Option<String>,
    #[serde(default)]
    include_archived: bool,
}
async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, HttpError> {
    let parent = query
        .from_session_id
        .as_deref()
        .map(|id| caller(&state, id))
        .transpose()?;
    let project = parent
        .map(|task| task.project_id)
        .or(query.project_id.clone())
        .ok_or_else(|| {
            crate::instances::invalid("Provide from_session_id or project_id to list tasks")
        })?;
    if query
        .project_id
        .as_ref()
        .is_some_and(|requested| requested != &project)
    {
        return Err(crate::instances::invalid(
            "Project does not match the calling Task",
        ));
    }
    sync_bindings(&state)?;
    let items: Vec<_> = state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .filter(|task| {
            task.project_id == project
                && (query.include_archived || !task.archived)
                && query
                    .parent_id
                    .as_ref()
                    .is_none_or(|id| task.parent_id.as_ref() == Some(id))
        })
        .map(Task::from)
        .collect();
    Ok(Json(json!({"items":items})))
}
async fn get_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, HttpError> {
    detail(&state, &id).map(Json)
}
#[derive(Deserialize)]
struct Submit {
    task: String,
    #[serde(default = "timeout_seconds")]
    timeout_seconds: u64,
}
async fn submit_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<Submit>,
) -> Result<Json<Value>, HttpError> {
    validate_timeout(request.timeout_seconds)?;
    if request.task.trim().is_empty() || request.task.len() > 131_072 {
        return Err(crate::instances::invalid(
            "Task text must contain 1-131072 bytes",
        ));
    }
    let task = task(&state, &id)?;
    if task.archived {
        return Err(conflict("Cannot submit to an archived Task"));
    }
    let mut submission =
        crate::acp::start_task(state.clone(), task.thread_id.clone(), Some(request.task))?;
    tokio::time::timeout(
        Duration::from_secs(request.timeout_seconds),
        &mut submission,
    )
    .await
    .map_err(|_| {
        conflict("Submission timed out and may still execute; inspect task get before retrying")
    })?
    .map_err(|error| HttpError::internal(error.to_string()))??;
    detail(&state, &id).map(Json)
}
async fn archive_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, HttpError> {
    task(&state, &id)?;
    finish(state.clone(), id.clone(), true, Finish::default(), true).await?;
    detail(&state, &id).map(Json)
}
async fn delete_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, HttpError> {
    task(&state, &id)?;
    finish(state, id.clone(), false, Finish::default(), true).await?;
    Ok(Json(json!({"id":id,"deleted":true})))
}
