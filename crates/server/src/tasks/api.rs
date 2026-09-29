use super::{execution, store::*};
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
        .route("/inbox", get(list_inbox).post(write_inbox))
        .route("/inbox/{id}", get(get_inbox))
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
async fn write_inbox(
    State(state): State<AppState>,
    Json(input): Json<InboxWrite>,
) -> Result<Json<InboxItem>, HttpError> {
    state.tasks.write_inbox(input).map(Json)
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
    if input.statuses.is_empty() {
        return Err(invalid("The board needs at least one status"));
    }
    let mut ids = std::collections::HashSet::new();
    for status in &input.statuses {
        identifier(&status.id)?;
        text(&status.name, 128)?;
        if !ids.insert(&status.id) {
            return Err(invalid("Status IDs must be unique"));
        }
        if status.color.len() != 7
            || !status.color.starts_with('#')
            || !status.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("Use a six-digit hex color"));
        }
    }
    state
        .tasks
        .change(|board| {
            revision(board.status_revision, input.expected_revision)?;
            if board.tasks.iter().any(|t| !ids.contains(&t.status_id)) {
                return Err(conflict(
                    "Cannot remove a status used by a task. Move its tasks first.",
                ));
            }
            board.statuses = input.statuses.clone();
            board.status_revision += 1;
            Ok(board.clone())
        })
        .map(Json)
}
async fn convert(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TaskConvert>,
) -> Result<Json<BoardTask>, HttpError> {
    // Retry of an accepted conversion never creates another terminal/worktree.
    if let Ok(task) = state.tasks.get(&input.id) {
        if task.inbox_id == id && task.project_id == input.project_id {
            return Ok(Json(task));
        }
        return Err(conflict("Task ID already exists"));
    }
    identifier(&input.id)?;
    text(&input.title, 512)?;
    if !input.description.is_empty() {
        text(&input.description, 100_000)?;
    }
    let item = state.tasks.get_inbox(&id)?;
    check_project(&item, &input.project_id)?;
    execution::validate(&state, &input).await?;
    let task = state.tasks.create_task(
        BoardTask {
            id: input.id.clone(),
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
    )?;
    tokio::spawn(execution::prepare(state, input));
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
