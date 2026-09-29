use super::{execution, requests::CreationRequest, store::*};
use crate::{AppState, HttpError};
use aow_protocol::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(board))
        .route("/statuses", post(write_statuses))
        .route("/inbox", get(list_inbox).post(create_inbox))
        .route("/inbox/{id}", get(get_inbox).post(update_inbox))
        .route("/inbox/{id}/delete", post(delete_inbox))
        .route("/inbox/{id}/convert", post(convert))
        .route("/items/{id}", get(get_task))
        .route("/items/{id}/status", post(move_task))
        .route("/items/{id}/start", post(execution::start))
        .route("/items/{id}/archive", post(archive))
}
#[derive(serde::Deserialize)]
struct BoardQuery {
    project_id: Option<String>,
}
async fn board(
    State(state): State<AppState>,
    Query(query): Query<BoardQuery>,
) -> Result<Json<TaskBoard>, HttpError> {
    let mut board = state.tasks.snapshot()?;
    if let Some(project_id) = query.project_id {
        board.tasks.retain(|task| task.project_id == project_id);
    }
    Ok(Json(board))
}
async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BoardTask>, HttpError> {
    state.tasks.get(&id).map(Json)
}
#[derive(serde::Deserialize)]
struct InboxQuery {
    project_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    cursor: Option<String>,
    #[serde(default)]
    include_converted: bool,
}
fn default_limit() -> usize {
    50
}
async fn list_inbox(
    State(state): State<AppState>,
    Query(query): Query<InboxQuery>,
) -> Result<Json<InboxPage>, HttpError> {
    state
        .tasks
        .inbox_page(
            query.project_id.as_deref(),
            query.include_converted,
            query.limit,
            query.cursor.as_deref(),
        )
        .map(Json)
}
async fn get_inbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<InboxItem>, HttpError> {
    state.tasks.get_inbox(&id).map(Json)
}
async fn create_inbox(
    State(state): State<AppState>,
    Json(input): Json<InboxCreate>,
) -> Result<Json<InboxItem>, HttpError> {
    state.tasks.create_inbox(input).map(Json)
}
async fn update_inbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<InboxUpdate>,
) -> Result<Json<InboxItem>, HttpError> {
    state
        .tasks
        .write_inbox(InboxWrite {
            id,
            project_id: input.project_id,
            title: input.title,
            description: input.description,
            expected_revision: Some(input.expected_revision),
        })
        .map(Json)
}
async fn delete_inbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TaskRevision>,
) -> Result<Json<serde_json::Value>, HttpError> {
    state.tasks.delete_inbox(&id, input.expected_revision)?;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn write_statuses(
    State(state): State<AppState>,
    Json(input): Json<TaskStatusesWrite>,
) -> Result<Json<TaskBoard>, HttpError> {
    state.tasks.write_statuses(input).map(Json)
}
async fn convert(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut input): Json<TaskConvert>,
) -> Result<Json<BoardTask>, HttpError> {
    let request = CreationRequest::new(&input.request_key, &(&id, &input))?;
    if let Some(task) = state.tasks.task_for_request(&request)? {
        return Ok(Json(task));
    }
    text(&input.title, 512)?;
    if !input.description.is_empty() {
        text(&input.description, 100_000)?;
    }
    let item = state.tasks.get_inbox(&id)?;
    check_project(&item, &input.project_id)?;
    let task_id = aow_id::new_id();
    execution::resolve_worktree_defaults(&state, &mut input, &task_id)?;
    execution::validate(&state, &input).await?;
    let (task, created) = state.tasks.create_task(
        BoardTask {
            id: task_id,
            revision: 1,
            inbox_id: id,
            status_id: input.status_id.clone(),
            title: input.title.trim().into(),
            description: input.description.clone(),
            project_id: input.project_id.clone(),
            cwd: input.cwd.clone(),
            agent: input.agent.clone(),
            tab_id: None,
            pane_id: None,
            execution: TaskExecutionPhase::Preparing,
            error: None,
            archived: false,
            created_at: now(),
            updated_at: now(),
            history: vec![TaskStatusChange {
                status_id: input.status_id.clone(),
                at: now(),
                reason: "Created from Inbox".into(),
            }],
        },
        input.expected_revision,
        Some(request),
    )?;
    if created {
        tokio::spawn(execution::prepare(state, task.id.clone(), input));
    }
    Ok(Json(task))
}

fn check_project(item: &InboxItem, project_id: &str) -> Result<(), HttpError> {
    if item.project_id != project_id {
        return Err(conflict("This requirement belongs to a different project"));
    }
    Ok(())
}
async fn move_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TaskMove>,
) -> Result<Json<BoardTask>, HttpError> {
    if input.reason.len() > 4000 {
        return Err(invalid("Reason exceeds 4000 bytes"));
    }
    state
        .tasks
        .change(|board| {
            let task = board
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(missing)?;
            revision(task.revision, input.expected_revision)?;
            if !board.statuses.iter().any(|s| s.id == input.status_id) {
                return Err(invalid("Unknown task status"));
            }
            if task.status_id != input.status_id {
                task.status_id = input.status_id;
                task.history.push(TaskStatusChange {
                    status_id: task.status_id.clone(),
                    at: now(),
                    reason: input.reason,
                });
                task.revision += 1;
                task.updated_at = now();
            }
            Ok(task.clone())
        })
        .map(Json)
}
async fn archive(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TaskRevision>,
) -> Result<Json<BoardTask>, HttpError> {
    state
        .tasks
        .change(|board| {
            let task = board
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(missing)?;
            revision(task.revision, input.expected_revision)?;
            task.archived = !task.archived;
            task.revision += 1;
            task.updated_at = now();
            Ok(task.clone())
        })
        .map(Json)
}
