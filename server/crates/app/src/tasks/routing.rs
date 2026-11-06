//! Shared task collaboration for every ACP task conversation, including the project Orchestrator.
use super::api;
use super::api::{Create, WorkspaceMode};
use super::model::{Task, storage};
use crate::error::service_http_error;
use crate::instances::invalid;
use crate::tasks::TaskService;
use crate::{AppState, HttpError};
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
        .route("/v1/tasks/{id}/send", post(super::messages::send))
        .route("/v1/tasks/{id}/status", post(super::messages::status))
        .route("/v1/tasks/{id}/report", post(super::messages::report))
}

impl TaskService {
    pub(crate) async fn task_policy_for_thread(
        &self,
        service: &aow_host_client::acp::AcpClient,
        thread: &str,
    ) -> anyhow::Result<Option<aow_db::tasks::ConversationPolicy>> {
        let Some(task) = self.task_for_thread(service, thread).await? else {
            return Ok(None);
        };
        let mut execution = (&task).into();
        let mut current = thread.to_owned();
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current.clone()) {
            if self
                .task_for_thread(service, &current)
                .await?
                .is_none_or(|owner| owner.id != task.id)
            {
                break;
            }
            if let Some(recorded) = self.inner.tasks.conversation_policy(&current)? {
                execution = recorded;
                break;
            }
            let info = service.session_info(&current).await?;
            match info
                .parent
                .filter(|parent| parent.kind == aow_host_contract::instance::InstanceKind::Acp)
            {
                Some(parent) => current = parent.id,
                None => break,
            }
        }
        let info = service.session_info(thread).await?;
        execution.workspace = info.cwd;
        Ok(Some(execution))
    }

    pub(crate) async fn task_for_thread(
        &self,
        service: &aow_host_client::acp::AcpClient,
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
            let info = match service.session_info(&current).await {
                Ok(info) => info,
                Err(error) if current == thread => return Err(error),
                Err(_) => break,
            };
            match info
                .parent
                .filter(|parent| parent.kind == aow_host_contract::instance::InstanceKind::Acp)
            {
                Some(parent) => current = parent.id,
                None => break,
            }
        }
        Ok(None)
    }
}

pub(super) async fn source_task(state: &AppState, thread: &str) -> Result<TaskRecord, HttpError> {
    state
        .acp
        .session_info(thread)
        .await
        .map_err(crate::acp::task_error)?;
    state
        .tasks
        .task_for_thread(&state.acp, thread)
        .await
        .map_err(storage)?
        .filter(|task| !task.archived)
        .ok_or_else(|| invalid("The source conversation must belong to an active task"))
}

pub(super) fn find_task(state: &AppState, id: &str) -> Result<TaskRecord, HttpError> {
    state
        .tasks
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
#[serde(deny_unknown_fields)]
pub(super) struct Scope {
    source_thread_id: Option<String>,
}

async fn check_scope(state: &AppState, scope: &Scope, task: &TaskRecord) -> Result<(), HttpError> {
    if let Some(thread) = &scope.source_thread_id
        && source_task(state, thread).await?.project_id != task.project_id
    {
        return Err(invalid("Task is outside the source project"));
    }
    Ok(())
}

async fn roles(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let roles = state.roles.agent_roles().map_err(service_http_error)?;
    Ok(Json(json!({"items":roles.iter().map(|role| json!({
        "id":role.id, "name":role.config.name, "description":role.config.description,
        "runtime":role.config.runtime,
        "mention":format!("mention://role/{}", role.id),
        "collaboration_supported":matches!(role.config.runtime, crate::roles::RoleRuntime::Acp {..})
    })).collect::<Vec<_>>()})))
}

async fn role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let role = state.roles.agent_role(&id).map_err(service_http_error)?;
    Ok(Json(json!(role)))
}

async fn list(
    State(state): State<AppState>,
    Query(scope): Query<Scope>,
) -> Result<Json<Value>, HttpError> {
    let source = match scope.source_thread_id.as_deref() {
        Some(id) => Some(source_task(&state, id).await?),
        None => None,
    };
    let tasks = state
        .tasks
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
    let task = find_task(&state, &id)?;
    check_scope(&state, &scope, &task).await?;
    let mut conversations = Vec::new();
    for session in state
        .acp
        .sessions(None)
        .await
        .map_err(crate::acp::task_error)?
    {
        if state
            .tasks
            .task_for_thread(&state.acp, &session.thread_id)
            .await
            .map_err(storage)?
            .is_some_and(|owner| owner.id == id)
        {
            conversations.push(session);
        }
    }
    let delegated = state
        .tasks
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .filter(|child| child.source_task_id.as_deref() == Some(&id))
        .map(Task::from)
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"task":Task::from(task), "conversations":conversations, "delegated":delegated}),
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
    let source = source_task(&state, &request.source_thread_id).await?;
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
            if target.archived || target.project_id != source.project_id || target.is_orchestrator {
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
            .await
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
            is_orchestrator: false,
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
    if (request.is_orchestrator || request.workspace_intent.as_deref() == Some("analyze"))
        && workspace != main_workspace(state, &request.project_id).await?
    {
        return Err(invalid(
            "Orchestrator and analysis tasks use the project workspace",
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
                || target.is_orchestrator
            {
                return Err(invalid("Invalid review workspace or target"));
            }
            let root = std::path::Path::new(workspace);
            if !state
                .node
                .git_output(root, &["status", "--porcelain"])
                .await
                .map_err(service_http_error)?
                .trim()
                .is_empty()
            {
                return Err(invalid(
                    "Commit the target task's changes before requesting a review of its revision",
                ));
            }
            return state
                .node
                .git_output(root, &["rev-parse", "HEAD"])
                .await
                .map(|revision| Some(revision.trim().into()))
                .map_err(service_http_error);
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
        .projects
        .project(project_id)
        .await
        .map_err(service_http_error)?
        .worktrees
        .into_iter()
        .find(|worktree| worktree.is_main)
        .map(|worktree| worktree.path)
        .ok_or_else(|| invalid("Project has no main workspace"))
}

#[derive(Deserialize)]
pub(super) struct OrchestratorRequest {
    #[serde(default)]
    agent_id: String,
    agent_role_id: Option<String>,
}

pub(super) async fn orchestrator(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(request): Json<OrchestratorRequest>,
) -> Result<Json<Task>, HttpError> {
    if let Some(orchestrator) = state
        .tasks
        .inner
        .tasks
        .list()
        .map_err(storage)?
        .into_iter()
        .find(|task| task.project_id == project_id && task.is_orchestrator)
    {
        return Ok(Json(orchestrator.into()));
    }
    let workspace = main_workspace(&state, &project_id).await?;
    api::create(
        State(state),
        Json(Create {
            // A deleted space's messages/history keep their old task identity.
            request_id: format!("orchestrator-{project_id}-{}", uuid::Uuid::new_v4()),
            project_id,
            prompt: "Orchestrator".into(),
            agent_id: request.agent_id,
            agent_role_id: request.agent_role_id,
            workspace_mode: WorkspaceMode::Existing,
            workspace: Some(workspace),
            slug: None,
            dependencies: vec![],
            is_orchestrator: true,
            source_thread_id: None,
            workspace_intent: None,
            target_task_id: None,
        }),
    )
    .await
}
