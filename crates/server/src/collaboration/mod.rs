//! Durable task execution and return delivery. Broadcasts are hints, never the queue.
#[cfg(test)]
mod tests;
mod worker;

use crate::AppState;
use anyhow::Result;
use aow_db::tasks::{RunSource, TaskRecord, TaskRun};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub(crate) struct Runtime(Arc<Inner>);

#[derive(Default)]
struct Inner {
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    notify: Arc<Notify>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().unwrap().take() {
            task.abort();
        }
    }
}

impl Runtime {
    pub(crate) fn start(&self, state: &AppState) {
        let mut task = self.0.task.lock().unwrap();
        if task.is_none() {
            // Do not capture AppState/Runtime here: that would keep the worker alive
            // after the server and its last consumer have gone away.
            *task = Some(aow_i18n::spawn(worker::run(
                worker::Worker {
                    manager: state.aow.clone(),
                    acp: state.acp.clone(),
                    controls: state.acp_controls.clone(),
                    events: state.workspace_events.clone(),
                },
                self.0.notify.clone(),
            )));
        }
        self.0.notify.notify_one();
    }
}

pub(crate) fn source(state: &AppState, task: &TaskRecord, thread: &str) -> Result<RunSource> {
    let snapshot = state.acp.snapshot(thread)?;
    let turn = snapshot.turns.last();
    let start = turn.and_then(|turn| {
        snapshot
            .entries
            .iter()
            .position(|entry| entry.id == turn.user_entry_id)
    });
    let entries = start.map_or(&[][..], |start| &snapshot.entries[start..]);
    let request = entries
        .iter()
        .take_while(|entry| entry.kind == "user")
        .filter(|entry| entry.content["_meta"]["aow/instructions"] != true)
        .filter_map(|entry| entry.content["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let message_id = entries
        .iter()
        .find(|entry| entry.kind == "user" && entry.content["_meta"]["aow/instructions"] != true)
        .map(|entry| entry.id.clone());
    let parent_run_id = turn
        .filter(|turn| turn.status == "in_progress")
        .map(|turn| state.aow.task_db().run_for_prompt(&turn.id))
        .transpose()?
        .flatten();
    Ok(RunSource {
        task_id: task.id.clone(),
        thread_id: thread.into(),
        turn_id: turn.map(|turn| turn.id.clone()).filter(|id| !id.is_empty()),
        message_id,
        request,
        parent_run_id,
    })
}

pub(crate) fn new_run(
    task: &TaskRecord,
    request_id: String,
    prompt: String,
    source: Option<RunSource>,
) -> TaskRun {
    TaskRun {
        id: uuid::Uuid::new_v4().to_string(),
        request_id,
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        thread_id: task.initial_thread_id.clone().expect("ACP task"),
        prompt,
        source,
        state: "queued".into(),
        result: None,
        error: None,
        created_at: now(),
        finished_at: None,
        attempts: 0,
        retry_at: 0,
    }
}

pub(super) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
