//! Explicit reports, independent of ACP turn completion and task acceptance.
use super::{DeliveryState, MessageDelivery, MessageSource, TaskDb, messages};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Progress,
    Result,
    Blocked,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTarget {
    pub task_id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEvent {
    pub id: String,
    pub request_id: String,
    pub project_id: String,
    pub task_id: String,
    pub thread_id: String,
    pub kind: EventKind,
    pub message: String,
    pub target: Option<EventTarget>,
    pub source: Option<MessageSource>,
    pub created_at: i64,
}

impl TaskDb {
    pub fn event_for_request(&self, thread: &str, request: &str) -> Result<Option<TaskEvent>> {
        let data: Option<String> = self
            .lock()?
            .query_row(
                "SELECT payload FROM task_events WHERE thread_id=?1 AND request_id=?2",
                params![thread, request],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }

    pub fn event(&self, id: &str) -> Result<Option<TaskEvent>> {
        let data: Option<String> = self
            .lock()?
            .query_row("SELECT payload FROM task_events WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }

    pub fn publish_event(&self, event: &TaskEvent) -> Result<TaskEvent> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM task_events WHERE thread_id=?1 AND request_id=?2",
                params![event.thread_id, event.request_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let existing: TaskEvent = serde_json::from_str(&existing)?;
            ensure!(
                existing.task_id == event.task_id
                    && existing.kind == event.kind
                    && existing.message == event.message
                    && existing.target == event.target,
                "Request ID already used for another report"
            );
            return Ok(existing);
        }
        tx.execute("INSERT INTO task_events (id,project_id,thread_id,request_id,created_at,payload) VALUES (?1,?2,?3,?4,?5,?6)", params![event.id,event.project_id,event.thread_id,event.request_id,event.created_at,serde_json::to_string(event)?])?;
        if let Some(target) = &event.target {
            ensure!(
                target.thread_id != event.thread_id,
                "A report cannot wake its own conversation"
            );
            messages::insert(
                &tx,
                &MessageDelivery {
                    id: format!("inbox-{}", event.id),
                    request_id: format!("inbox-{}", event.id),
                    task_id: target.task_id.clone(),
                    thread_id: target.thread_id.clone(),
                    prompt: String::new(),
                    source: event.source.clone(),
                    event_id: Some(event.id.clone()),
                    state: DeliveryState::Pending,
                    created_at: event.created_at,
                    attempts: 0,
                    retry_at: 0,
                    error: None,
                },
            )?;
        }
        tx.commit()?;
        Ok(event.clone())
    }
}
