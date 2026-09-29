use super::persistence::Persistence;
use super::requests::{CreationReceipt, CreationRequest};
use crate::{HttpError, workspace_events::WorkspaceEvents};
use aow_protocol::*;
use axum::http::StatusCode;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(crate) struct TaskStore {
    pub(super) document: Arc<Mutex<TaskData>>,
    pub(super) persistence: Option<Persistence>,
    pub(super) events: WorkspaceEvents,
}

/// Internal write after the server has resolved the resource identity.
#[derive(Clone)]
pub(super) struct InboxWrite {
    pub id: String,
    pub project_id: String,
    pub expected_revision: Option<u64>,
    pub title: String,
    pub description: String,
}

pub(super) fn invalid(message: impl Into<String>) -> HttpError {
    HttpError::new(StatusCode::BAD_REQUEST, "invalid_task", message, None)
}
pub(super) fn conflict(message: impl Into<String>) -> HttpError {
    HttpError::new(StatusCode::CONFLICT, "task_conflict", message, None)
}
pub(super) fn missing() -> HttpError {
    HttpError::new(
        StatusCode::NOT_FOUND,
        "task_not_found",
        "Task or Inbox item no longer exists",
        None,
    )
}
pub(super) fn revision(actual: u64, expected: u64) -> Result<(), HttpError> {
    if actual != expected {
        return Err(conflict(
            "This item changed. Refresh it before trying again.",
        ));
    }
    Ok(())
}
pub(super) fn text(value: &str, max: usize) -> Result<(), HttpError> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(invalid(format!(
            "Text must contain 1-{max} bytes without control characters"
        )));
    }
    Ok(())
}
pub(super) fn identifier(value: &str) -> Result<(), HttpError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(invalid(
            "IDs must contain 1-128 letters, digits, underscores or hyphens",
        ));
    }
    Ok(())
}
pub(super) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub(super) struct TaskData {
    pub sources: BTreeMap<String, super::sources::SourceSettings>,
    pub board: TaskBoard,
    /// Persistent stores retain only summaries. Bodies are read on demand.
    pub inbox: BTreeMap<String, InboxSummary>,
    pub memory_inbox: BTreeMap<String, InboxItem>,
    pub inbox_requests: BTreeMap<String, CreationReceipt>,
    pub task_requests: BTreeMap<String, CreationReceipt>,
    pub status_request: Option<CreationRequest>,
}

impl TaskStore {
    pub(crate) fn new(
        state_dir: Option<&Path>,
        config: Option<aow_config::ConfigRepository>,
        events: WorkspaceEvents,
    ) -> anyhow::Result<Self> {
        let persistence = match (state_dir, config) {
            (Some(state_dir), Some(config)) => Some(Persistence::new(state_dir, config)),
            (None, None) => None,
            _ => anyhow::bail!("Persistent tasks require an active configuration"),
        };
        let mut data = TaskData {
            sources: BTreeMap::new(),
            board: defaults(),
            inbox: BTreeMap::new(),
            memory_inbox: BTreeMap::new(),
            inbox_requests: BTreeMap::new(),
            task_requests: BTreeMap::new(),
            status_request: None,
        };
        if let Some(persistence) = &persistence {
            persistence.load(&mut data)?;
        }
        let mut interrupted = false;
        for task in &mut data.board.tasks {
            if matches!(
                task.execution,
                TaskExecutionPhase::Preparing | TaskExecutionPhase::Submitting
            ) {
                task.execution = TaskExecutionPhase::Failed;
                task.error = Some("AoW restarted during setup or submission. Inspect the linked terminal and worktree before continuing; delivery may have occurred.".into());
                task.revision += 1;
                task.updated_at = now();
                interrupted = true;
            }
        }
        if interrupted && let Some(persistence) = &persistence {
            persistence.save_tasks(&data.board.tasks, &data.task_requests)?;
        }
        Ok(Self {
            document: Arc::new(Mutex::new(data)),
            persistence,
            events,
        })
    }
    pub(crate) fn snapshot(&self) -> Result<TaskBoard, HttpError> {
        Ok(self.lock()?.board.clone())
    }
    pub(super) fn lock(&self) -> Result<std::sync::MutexGuard<'_, TaskData>, HttpError> {
        self.document
            .lock()
            .map_err(|e| HttpError::internal(e.to_string()))
    }
    pub(super) fn change<T>(
        &self,
        change: impl FnOnce(&mut TaskBoard) -> Result<T, HttpError>,
    ) -> Result<T, HttpError> {
        let mut current = self.lock()?;
        let mut next = current.board.clone();
        let result = change(&mut next)?;
        self.save_board(&current, &next)?;
        if current.board.status_revision != next.status_revision {
            current.status_request = None;
        }
        current.board = next;
        drop(current);
        self.events.tasks_changed();
        Ok(result)
    }
    pub(super) fn save_board(&self, current: &TaskData, next: &TaskBoard) -> Result<(), HttpError> {
        if let Some(persistence) = &self.persistence {
            if current.board.status_revision != next.status_revision
                || current.board.statuses != next.statuses
            {
                persistence
                    .save_statuses(next, None)
                    .map_err(|e| HttpError::internal(format!("{e:#}")))?;
            }
            if current.board.tasks != next.tasks {
                persistence
                    .save_tasks(&next.tasks, &current.task_requests)
                    .map_err(|e| HttpError::internal(format!("{e:#}")))?;
            }
        }
        Ok(())
    }
    pub(crate) fn update_execution(
        &self,
        id: &str,
        change: impl FnOnce(&mut BoardTask),
    ) -> Result<BoardTask, HttpError> {
        self.change(|board| {
            let task = board
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(missing)?;
            change(task);
            task.revision += 1;
            task.updated_at = now();
            Ok(task.clone())
        })
    }
    pub(super) fn get(&self, id: &str) -> Result<BoardTask, HttpError> {
        self.snapshot()?
            .tasks
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(missing)
    }
}
pub(super) fn defaults() -> TaskBoard {
    TaskBoard {
        version: 1,
        tasks: vec![],
        status_revision: 1,
        statuses: [
            ("todo", "Todo", "#8b8b93"),
            ("in-progress", "In progress", "#d7a84b"),
            ("in-review", "In review", "#7999e8"),
            ("done", "Done", "#62b58d"),
        ]
        .into_iter()
        .map(|(id, name, color)| TaskStatus {
            id: id.into(),
            name: name.into(),
            color: color.into(),
        })
        .collect(),
    }
}
