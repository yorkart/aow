//! Schedule another execution of a business task, with its own role and workspace.
use super::{
    model::{Task, storage, validate_prompt},
    routing::{find_task, main_workspace, source_task},
    workspace,
};
use crate::{
    AppState, HttpError,
    aow::{
        aow_http_error,
        roles::{RoleRuntime, RoleSession},
    },
    instances::invalid,
};
use aow_db::tasks::{RunExecution, TaskRecord};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub(super) struct Start {
    request_id: String,
    source_thread_id: String,
    prompt: String,
    agent_role_id: Option<String>,
    intent: Option<String>,
    target_task_id: Option<String>,
}

pub(super) async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Start>,
) -> Result<Json<Value>, HttpError> {
    validate_prompt(&request.prompt)?;
    if request.request_id.is_empty()
        || request.request_id.len() > 128
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid("Invalid run request ID"));
    }
    // Provisioning and persistence survive a disconnected CLI/browser, as with task creation.
    aow_i18n::spawn(start_owned(state, id, request))
        .await
        .map_err(|error| HttpError::internal(error.to_string()))?
}

async fn start_owned(
    state: AppState,
    id: String,
    request: Start,
) -> Result<Json<Value>, HttpError> {
    let _operation = state.aow.task_resource_access().await;
    let task = find_task(&state, &id)?;
    let source = source_task(&state, &request.source_thread_id)?;
    if task.archived || task.project_id != source.project_id {
        return Err(invalid(
            "Target must be an active task in the source project",
        ));
    }
    let db = state.aow.task_db();
    let request_id = format!("send-{}", request.request_id);
    let role_id = request.agent_role_id.clone().or(task.agent_role_id.clone());
    let intent = request.intent.clone().or(task.workspace_intent.clone());
    let target_id = if intent.as_deref() == Some("review") {
        Some(
            request
                .target_task_id
                .clone()
                .or(task.target_task_id.clone())
                .unwrap_or(id.clone()),
        )
    } else {
        if request.target_task_id.is_some() {
            return Err(invalid("target_task_id is only used for reviews"));
        }
        None
    };
    if let Some(existing) = db.run_for_request(&id, &request_id).map_err(storage)? {
        if existing.prompt != request.prompt
            || existing.source.as_ref().map(|s| &s.thread_id) != Some(&request.source_thread_id)
            || existing.execution.as_ref().is_none_or(|e| {
                e.agent_role_id != role_id
                    || e.workspace_intent != intent
                    || e.target_task_id != target_id
            })
        {
            return Err(invalid("Request ID already used for different work"));
        }
        return Ok(Json(
            json!({"task_id":id,"thread_id":existing.thread_id,"accepted":true,"run_id":existing.id,"state":existing.state,"return_delivery":"inbox_and_wakeup"}),
        ));
    }
    if task.is_router && (request.intent.is_some() || request.agent_role_id.is_some()) {
        return Err(invalid(
            "Create an execution task for substantial work; Router identity is persistent",
        ));
    }
    if matches!(task.status.as_str(), "done" | "cancelled") {
        return Err(invalid("Reopen this task before scheduling more work"));
    }
    let run_source = crate::collaboration::source(&state, &source, &request.source_thread_id)
        .map_err(storage)?;
    let role = role_id
        .as_deref()
        .map(|id| state.aow.agent_role(id))
        .transpose()
        .map_err(aow_http_error)?;
    let agent_id = match &role {
        Some(role) => {
            state.aow.check_role_runtime(role, &state).await?;
            match &role.config.runtime {
                RoleRuntime::Acp { agent_id } => agent_id.clone(),
                _ => return Err(invalid("Task collaboration requires an ACP role")),
            }
        }
        None => task.agent_id.clone(),
    };
    if task.initial_thread_id.is_none() {
        return Err(invalid("This task has no ACP conversation"));
    }
    let _filesystem = state.aow.filesystem_access().await;
    let (workspace, revision) = if request.intent.is_none() {
        (task.workspace.clone(), task.target_revision.clone())
    } else {
        match intent.as_deref() {
            None => (task.workspace.clone(), None),
            Some("analyze") => (main_workspace(&state, &task.project_id).await?, None),
            Some("change") => {
                let main = main_workspace(&state, &task.project_id).await?;
                let workspace = change_workspace(&state, &task)?;
                let borrowed_review_workspace = task.workspace_intent.as_deref() == Some("review")
                    && db
                        .execution_workspace(&task.id, "change")
                        .map_err(storage)?
                        .is_none();
                let workspace = if workspace == main || borrowed_review_workspace {
                    workspace::create_task_workspace(
                        &state,
                        &task.project_id,
                        &task.id,
                        task.slug.as_deref(),
                    )
                    .await
                    .map_err(aow_http_error)?
                } else {
                    workspace
                };
                (workspace, None)
            }
            Some("review") => {
                let target = find_task(&state, target_id.as_deref().unwrap())?;
                if target.archived || target.is_router || target.project_id != task.project_id {
                    return Err(invalid("Invalid review target"));
                }
                let workspace = change_workspace(&state, &target)?;
                let root = std::path::Path::new(&workspace);
                if !crate::aow::git_output(root, &["status", "--porcelain"])
                    .await
                    .map_err(aow_http_error)?
                    .trim()
                    .is_empty()
                {
                    return Err(invalid(
                        "Commit changes before requesting a review of their revision",
                    ));
                }
                let revision = crate::aow::git_output(root, &["rev-parse", "HEAD"])
                    .await
                    .map_err(aow_http_error)?;
                (workspace, Some(revision.trim().into()))
            }
            _ => return Err(invalid("intent must be change, analyze or review")),
        }
    };
    let workspace = if workspace == task.workspace {
        workspace
    } else {
        state
            .aow
            .agent_worktree_path(&task.project_id, &workspace)
            .await
            .map_err(aow_http_error)?
    };
    let execution = RunExecution {
        agent_id,
        agent_role_id: role_id,
        workspace,
        workspace_intent: intent,
        target_task_id: target_id,
        target_revision: revision,
    };
    // A conversation has one role and workspace policy. A different role or
    // review revision uses another thread; matching later runs reuse context.
    let mut thread = if execution == RunExecution::from(&task) {
        task.initial_thread_id.clone()
    } else {
        None
    };
    for resource in task.resources.iter().filter(|r| r.kind == "acp") {
        if db.thread_execution(&resource.id).map_err(storage)?.as_ref() == Some(&execution)
            && state.acp.session_info(&resource.id).is_ok()
        {
            thread = Some(resource.id.clone());
            break;
        }
    }
    if thread.as_deref() == Some(request.source_thread_id.as_str()) {
        return Err(invalid("Continue the current conversation directly"));
    }
    let session = if thread.is_none() {
        let session = if let Some(role) = &role {
            state
                .aow
                .prepare_role_session(role, &state, &execution.workspace, None)
                .await?
        } else {
            RoleSession::Acp {
                session: Box::new(
                    state
                        .acp
                        .prepare_session(&execution.agent_id, execution.workspace.clone().into())
                        .await
                        .map_err(crate::acp::task_error)?,
                ),
            }
        };
        thread = Some(session.resource().1.into());
        Some(session)
    } else {
        None
    };
    let thread = thread.unwrap();
    let mut run =
        crate::collaboration::new_run(&task, request_id, request.prompt, Some(run_source));
    run.thread_id = thread.clone();
    run.execution = Some(execution);
    if let Err(error) = db.enqueue_task_run(&run) {
        if let Some(session) = session {
            session.discard(&state).await;
        }
        return Err(invalid(error.to_string()));
    }
    state.collaboration.start(&state);
    Ok(Json(
        json!({"task_id":id,"thread_id":thread,"accepted":true,"run_id":run.id,"state":run.state,"return_delivery":"inbox_and_wakeup"}),
    ))
}

fn change_workspace(state: &AppState, task: &TaskRecord) -> Result<String, HttpError> {
    Ok(state
        .aow
        .task_db()
        .execution_workspace(&task.id, "change")
        .map_err(storage)?
        .unwrap_or_else(|| task.workspace.clone()))
}

#[derive(Deserialize)]
pub(super) struct Status {
    status: String,
    source_thread_id: Option<String>,
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Status>,
) -> Result<Json<Task>, HttpError> {
    let _operation = state.aow.task_resource_access().await;
    let mut task = find_task(&state, &id)?;
    if let Some(thread) = &request.source_thread_id
        && source_task(&state, thread)?.project_id != task.project_id
    {
        return Err(invalid("Task is outside the source project"));
    }
    state
        .aow
        .task_db()
        .set_status(&id, &request.status)
        .map_err(|error| invalid(error.to_string()))?;
    task.status = request.status;
    state.collaboration.start(&state);
    Ok(Json(task.into()))
}
