//! Shared task collaboration for every ACP task conversation, including the project Router.
use super::{
    api::{self, Create, WorkspaceMode},
    model::{Task, storage},
};
use crate::{
    AppState, HttpError,
    aow::{AowManager, aow_http_error},
    instances::invalid,
};
use aow_db::tasks::TaskRecord;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get as route_get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn cli_routes() -> Router<AppState> {
    Router::new()
        .route("/v1/roles", route_get(roles))
        .route("/v1/roles/{id}", route_get(role))
        .route("/v1/tasks", route_get(list).post(delegate))
        .route("/v1/tasks/{id}", route_get(self::get))
        .route("/v1/tasks/{id}/send", post(super::runs::start))
        .route("/v1/tasks/{id}/runs", post(super::runs::start))
        .route("/v1/tasks/{id}/status", post(super::runs::status))
}

impl AowManager {
    pub(crate) fn task_execution_for_thread(
        &self,
        service: &aow_acp::AcpService,
        thread: &str,
    ) -> anyhow::Result<Option<aow_db::tasks::RunExecution>> {
        let Some(task) = self.task_for_thread(service, thread)? else {
            return Ok(None);
        };
        let mut execution = (&task).into();
        let mut current = thread.to_owned();
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current.clone()) {
            if let Some(recorded) = self.task_db().thread_execution(&current)? {
                execution = recorded;
                break;
            }
            let info = service.session_info(&current)?;
            match info
                .parent
                .filter(|parent| parent.kind == aow_protocol::InstanceKind::Acp)
            {
                Some(parent) => current = parent.id,
                None => break,
            }
        }
        let info = service.session_info(thread)?;
        execution.workspace = info.cwd;
        execution.agent_id = info.agent_id;
        execution.agent_role_id = self.acp_role_id(service, thread)?;
        Ok(Some(execution))
    }

    pub(crate) fn task_for_thread(
        &self,
        service: &aow_acp::AcpService,
        thread: &str,
    ) -> anyhow::Result<Option<TaskRecord>> {
        let tasks = self.inner.tasks.list()?;
        let mut current = thread.to_owned();
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current.clone()) {
            if let Some(task) = tasks.iter().find(|task| {
                task.resources
                    .iter()
                    .any(|r| r.kind == "acp" && r.id == current)
            }) {
                return Ok(Some(task.clone()));
            }
            let info = match service.session_info(&current) {
                Ok(info) => info,
                Err(error) if current == thread => return Err(error),
                Err(_) => break,
            };
            match info
                .parent
                .filter(|parent| parent.kind == aow_protocol::InstanceKind::Acp)
            {
                Some(parent) => current = parent.id,
                None => break,
            }
        }
        Ok(None)
    }
}

pub(super) fn source_task(state: &AppState, thread: &str) -> Result<TaskRecord, HttpError> {
    state
        .acp
        .session_info(thread)
        .map_err(crate::acp::task_error)?;
    state
        .aow
        .task_for_thread(&state.acp, thread)
        .map_err(storage)?
        .filter(|task| !task.archived)
        .ok_or_else(|| invalid("The source conversation must belong to an active task"))
}

pub(super) fn find_task(state: &AppState, id: &str) -> Result<TaskRecord, HttpError> {
    state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| {
            HttpError::new(
                axum::http::StatusCode::NOT_FOUND,
                "task_not_found",
                "Task not found",
                None,
            )
        })
}

#[derive(Deserialize, Default)]
pub(super) struct Scope {
    source_thread_id: Option<String>,
    run_id: Option<String>,
}

fn check_scope(state: &AppState, scope: &Scope, task: &TaskRecord) -> Result<(), HttpError> {
    if let Some(thread) = &scope.source_thread_id
        && source_task(state, thread)?.project_id != task.project_id
    {
        return Err(invalid("Task is outside the source project"));
    }
    Ok(())
}

async fn roles(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let roles = state.aow.agent_roles().map_err(aow_http_error)?;
    Ok(Json(json!({"items":roles.iter().map(|role| json!({
        "id":role.id, "name":role.config.name, "description":role.config.description,
        "runtime":role.config.runtime,
        "mention":format!("mention://role/{}", role.id),
        "collaboration_supported":matches!(role.config.runtime, crate::aow::roles::RoleRuntime::Acp {..})
    })).collect::<Vec<_>>()})))
}

async fn role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let role = state.aow.agent_role(&id).map_err(aow_http_error)?;
    Ok(Json(json!(role)))
}

async fn list(
    State(state): State<AppState>,
    Query(scope): Query<Scope>,
) -> Result<Json<Value>, HttpError> {
    let source = scope
        .source_thread_id
        .as_deref()
        .map(|id| source_task(&state, id))
        .transpose()?;
    let tasks = state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .filter(|task| {
            source
                .as_ref()
                .is_none_or(|source| source.project_id == task.project_id)
        })
        .map(Task::from)
        .collect::<Vec<_>>();
    Ok(Json(json!({"items":tasks})))
}

pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(scope): Query<Scope>,
) -> Result<Json<Value>, HttpError> {
    if let Some(run_id) = &scope.run_id {
        let run = state
            .aow
            .task_db()
            .run(run_id)
            .map_err(storage)?
            .filter(|run| run.task_id == id)
            .ok_or_else(|| invalid("Run does not belong to this task"))?;
        if let Some(thread) = &scope.source_thread_id
            && source_task(&state, thread)?.project_id != run.project_id
        {
            return Err(invalid("Run is outside the source project"));
        }
        // Results survive task deletion and never consult a later conversation turn.
        let task_status = state.aow.task_db().status(&id).map_err(storage)?;
        return Ok(Json(json!({"run":run,"task_status":task_status})));
    }
    let task = find_task(&state, &id)?;
    check_scope(&state, &scope, &task)?;
    let runs = state.aow.task_db().task_runs(&id).map_err(storage)?;
    let runtime = runs.first().map(|run| run.thread_id.as_str()).or(task.initial_thread_id.as_deref()).map(|id| match state.acp.snapshot(id) {
        Ok(session) => {
            // Idle describes the adapter, not whether the assigned work succeeded.
            let start = session.entries.iter().rposition(|entry| entry.kind == "user").unwrap_or(0);
            let response = runs.first().map(|run| run.result.clone().unwrap_or_default()).unwrap_or_else(||
                session.entries.iter().skip(start).filter(|entry| entry.kind == "assistant")
                .filter_map(|entry| entry.content["text"].as_str()).collect::<Vec<_>>().join(""));
            let error = runs.first().map_or_else(|| session.error.clone(), |run| run.error.clone());
            json!({"thread_id":id, "status":session.status, "error":error,
                "pending_permissions":session.permissions.len(), "response":response, "updated_at":session.updated_at,
                "run_id":runs.first().map(|run| &run.id), "run_state":runs.first().map(|run| &run.state)})
        },
        Err(error) => json!({"thread_id":id, "status":"unavailable", "error":error.to_string()}),
    });
    let delegated = state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .filter(|child| child.source_task_id.as_deref() == Some(&id))
        .map(Task::from)
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"task":Task::from(task), "runtime":runtime, "runs":runs, "delegated":delegated}),
    ))
}

#[derive(Deserialize)]
struct Delegate {
    request_id: String,
    source_thread_id: String,
    prompt: String,
    agent_role_id: Option<String>,
    intent: String,
    target_task_id: Option<String>,
}

async fn delegate(
    State(state): State<AppState>,
    Json(request): Json<Delegate>,
) -> Result<Json<Task>, HttpError> {
    let source = source_task(&state, &request.source_thread_id)?;
    let (workspace_mode, workspace) = match request.intent.as_str() {
        "change" => (WorkspaceMode::NewWorktree, None),
        "analyze" => (
            WorkspaceMode::Existing,
            Some(main_workspace(&state, &source.project_id).await?),
        ),
        "review" => {
            let target = find_task(
                &state,
                request
                    .target_task_id
                    .as_deref()
                    .ok_or_else(|| invalid("Review requires target_task_id"))?,
            )?;
            if target.archived || target.project_id != source.project_id || target.is_router {
                return Err(invalid(
                    "Review target must be an active execution task in this project",
                ));
            }
            (WorkspaceMode::Existing, Some(target.workspace))
        }
        _ => return Err(invalid("intent must be change, analyze or review")),
    };
    let agent_id = if request.agent_role_id.is_some() {
        String::new()
    } else {
        state
            .acp
            .session_info(&request.source_thread_id)
            .map_err(crate::acp::task_error)?
            .agent_id
    };
    api::create(
        State(state),
        Json(Create {
            request_id: request.request_id,
            project_id: source.project_id,
            prompt: request.prompt,
            agent_id,
            agent_role_id: request.agent_role_id,
            workspace_mode,
            workspace,
            slug: None,
            dependencies: vec![],
            is_router: false,
            source_thread_id: Some(request.source_thread_id),
            workspace_intent: Some(request.intent),
            target_task_id: request.target_task_id,
        }),
    )
    .await
}

pub(super) async fn validate_workspace(
    state: &AppState,
    request: &Create,
    workspace: &str,
) -> Result<Option<String>, HttpError> {
    if (request.is_router || request.workspace_intent.as_deref() == Some("analyze"))
        && workspace != main_workspace(state, &request.project_id).await?
    {
        return Err(invalid(
            "Router and analysis tasks use the project workspace",
        ));
    }
    if request.source_thread_id.is_none()
        && (request.workspace_intent.is_some() || request.target_task_id.is_some())
    {
        return Err(invalid("Delegation requires a source conversation"));
    }
    match request.workspace_intent.as_deref() {
        Some("change") if !matches!(request.workspace_mode, WorkspaceMode::NewWorktree) => {
            return Err(invalid(
                "Code changes require an independent worktree; continue an existing task with send",
            ));
        }
        Some("review") => {
            let target = find_task(
                state,
                request
                    .target_task_id
                    .as_deref()
                    .ok_or_else(|| invalid("Review requires a target task"))?,
            )?;
            if target.project_id != request.project_id
                || target.workspace != workspace
                || target.archived
                || target.is_router
            {
                return Err(invalid("Invalid review workspace or target"));
            }
            let root = std::path::Path::new(workspace);
            if !super::super::git_output(root, &["status", "--porcelain"])
                .await
                .map_err(aow_http_error)?
                .trim()
                .is_empty()
            {
                return Err(invalid(
                    "Commit the target task's changes before requesting a review of its revision",
                ));
            }
            return super::super::git_output(root, &["rev-parse", "HEAD"])
                .await
                .map(|revision| Some(revision.trim().into()))
                .map_err(aow_http_error);
        }
        Some("change" | "analyze") | None => {}
        _ => return Err(invalid("Unknown workspace intent")),
    }
    if request.target_task_id.is_some() {
        return Err(invalid("target_task_id is only used for reviews"));
    }
    Ok(None)
}

pub(super) async fn main_workspace(
    state: &AppState,
    project_id: &str,
) -> Result<String, HttpError> {
    state
        .aow
        .project(project_id)
        .await
        .map_err(aow_http_error)?
        .worktrees
        .into_iter()
        .find(|worktree| worktree.is_main)
        .map(|worktree| worktree.path)
        .ok_or_else(|| invalid("Project has no main workspace"))
}

#[derive(Deserialize)]
pub(super) struct RouterRequest {
    #[serde(default)]
    agent_id: String,
    agent_role_id: Option<String>,
}

pub(super) async fn router(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<RouterRequest>,
) -> Result<Json<Task>, HttpError> {
    if let Some(router) = state
        .aow
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .find(|task| task.project_id == project_id && task.is_router)
    {
        return Ok(Json(router.into()));
    }
    let workspace = main_workspace(&state, &project_id).await?;
    api::create(
        State(state),
        Json(Create {
            request_id: format!("router-{project_id}"),
            project_id,
            prompt: "Router".into(),
            agent_id: request.agent_id,
            agent_role_id: request.agent_role_id,
            workspace_mode: WorkspaceMode::Existing,
            workspace: Some(workspace),
            slug: None,
            dependencies: vec![],
            is_router: true,
            source_thread_id: None,
            workspace_intent: None,
            target_task_id: None,
        }),
    )
    .await
}
