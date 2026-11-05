//! Conversation messages and explicit reports. A prompt ending is not a report.
use super::{
    model::{Task, storage, validate_prompt},
    routing::{find_task, source_task},
};
use crate::{AppState, HttpError, instances::invalid};
use aow_db::tasks::{EventKind, EventTarget, TaskEvent, TaskStatus};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

fn request_id(id: &str) -> Result<(), HttpError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid("Invalid request ID"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Send {
    request_id: String,
    source_thread_id: String,
    prompt: String,
    thread_id: Option<String>,
}

pub(super) async fn send(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Send>,
) -> Result<Json<Value>, HttpError> {
    validate_prompt(&request.prompt)?;
    request_id(&request.request_id)?;
    let _operation = state.tasks.task_resource_access().await;
    let task = find_task(&state, &id)?;
    let source = source_task(&state, &request.source_thread_id).await?;
    if task.archived || task.project_id != source.project_id {
        return Err(invalid(
            "Target must be an active task in the source project",
        ));
    }
    let thread = request
        .thread_id
        .or(task.initial_thread_id.clone())
        .ok_or_else(|| invalid("Task has no ACP conversation"))?;
    if source_task(&state, &thread).await?.id != id {
        return Err(invalid("Conversation does not belong to the target task"));
    }
    if thread == request.source_thread_id {
        return Err(invalid("Continue the current conversation directly"));
    }
    let origin = crate::collaboration::source(&state, &source, &request.source_thread_id)
        .await
        .map_err(storage)?;
    let mut message = crate::collaboration::new_message(
        &task,
        format!("send-{}", request.request_id),
        request.prompt,
        Some(origin),
    );
    message.thread_id = thread;
    let db = &state.tasks.inner.tasks;
    if db
        .message_for_request(&id, &message.request_id)
        .map_err(storage)?
        .is_none()
        && task.status.is_closed()
    {
        return Err(invalid("Reopen this task before sending work"));
    }
    let message = db
        .enqueue_message(&message)
        .map_err(|error| invalid(error.to_string()))?;
    state.collaboration.start(&state);
    Ok(Json(
        json!({"task_id":id,"thread_id":message.thread_id,"message_id":message.id,"accepted":true,"delivery":message.state}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    request_id: String,
    source_thread_id: String,
    kind: EventKind,
    message: String,
    target_thread_id: Option<String>,
}

pub(super) async fn report(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Report>,
) -> Result<Json<TaskEvent>, HttpError> {
    request_id(&request.request_id)?;
    validate_prompt(&request.message)?;
    let _operation = state.tasks.task_resource_access().await;
    let task = source_task(&state, &request.source_thread_id).await?;
    if task.id != id {
        return Err(invalid("Report source must belong to this task"));
    }
    let target = if let Some(thread) = request.target_thread_id {
        let target = source_task(&state, &thread).await?;
        if target.project_id != task.project_id {
            return Err(invalid("Report target must be in this project"));
        }
        Some(EventTarget {
            task_id: target.id,
            thread_id: thread,
        })
    } else {
        task.source_task_id
            .clone()
            .zip(task.source_thread_id.clone())
            .map(|(task_id, thread_id)| EventTarget { task_id, thread_id })
    };
    let source = crate::collaboration::report_source(&state, &task, &request.source_thread_id)
        .await
        .map_err(storage)?;
    let event = TaskEvent {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: request.request_id,
        project_id: task.project_id,
        task_id: task.id,
        thread_id: request.source_thread_id,
        kind: request.kind,
        message: request.message,
        target,
        source: Some(source),
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    let event = state
        .tasks
        .inner
        .tasks
        .publish_event(&event)
        .map_err(|error| invalid(error.to_string()))?;
    state.workspace_events.inbox_changed();
    state.collaboration.start(&state);
    Ok(Json(event))
}

#[derive(Deserialize)]
pub(super) struct Status {
    status: TaskStatus,
    source_thread_id: Option<String>,
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Status>,
) -> Result<Json<Task>, HttpError> {
    let _operation = state.tasks.task_resource_access().await;
    let mut task = find_task(&state, &id)?;
    if let Some(thread) = &request.source_thread_id
        && source_task(&state, thread).await?.project_id != task.project_id
    {
        return Err(invalid("Task is outside the source project"));
    }
    state
        .tasks
        .inner
        .tasks
        .set_status(&id, request.status)
        .map_err(|error| invalid(error.to_string()))?;
    task.status = request.status;
    if request.status == TaskStatus::Cancelled {
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
                && state
                    .acp
                    .snapshot(&session.thread_id)
                    .await
                    .is_ok_and(|snapshot| snapshot.status == "working")
            {
                state
                    .acp
                    .cancel(&session.thread_id)
                    .await
                    .map_err(crate::acp::task_error)?;
            }
        }
    }
    state.workspace_events.projects_changed();
    state.collaboration.start(&state);
    Ok(Json(task.into()))
}
