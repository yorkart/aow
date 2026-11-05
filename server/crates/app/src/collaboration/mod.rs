//! Durable conversation message delivery. Broadcasts are hints, never the queue.
#[cfg(test)]
mod tests;
mod worker;

use crate::AppState;
use anyhow::Result;
use aow_db::tasks::{DeliveryState, MessageDelivery, MessageSource, TaskRecord};
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
                    tasks: state.tasks.clone(),
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

pub(crate) async fn source(
    state: &AppState,
    task: &TaskRecord,
    thread: &str,
) -> Result<MessageSource> {
    let snapshot = state.acp.snapshot(thread).await?;
    let turn = snapshot.turns.last();
    let entries = turn
        .and_then(|turn| {
            snapshot
                .entries
                .iter()
                .position(|entry| entry.id == turn.user_entry_id)
        })
        .map_or(&[][..], |start| &snapshot.entries[start..]);
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
    let mut request = request;
    for entry in entries {
        if let Some(id) = entry.content["_meta"]["aow/event"]["id"].as_str()
            && let Some(event) = state.tasks.event(id)?
            && let Some(source) = event.source
        {
            request = source.request;
            break;
        }
    }
    Ok(MessageSource {
        task_id: task.id.clone(),
        thread_id: thread.into(),
        turn_id: turn.map(|turn| turn.id.clone()).filter(|id| !id.is_empty()),
        message_id,
        request,
    })
}

pub(crate) async fn report_source(
    state: &AppState,
    task: &TaskRecord,
    thread: &str,
) -> Result<MessageSource> {
    let snapshot = state.acp.snapshot(thread).await?;
    if let Some(turn) = snapshot.turns.last() {
        for (id, receipt) in &snapshot.prompt_receipts {
            if receipt.user_entry_id == turn.user_entry_id
                && let Some(message) = state.tasks.message_for_prompt(&task.id, thread, id)?
                && let Some(origin) = message.source
            {
                return Ok(origin);
            }
        }
    }
    // Older conversations can still contain a previously injected source prefix.
    if let Some(turn) = snapshot.turns.last()
        && let Some(start) = snapshot
            .entries
            .iter()
            .position(|entry| entry.id == turn.user_entry_id)
    {
        for entry in snapshot.entries[start..]
            .iter()
            .take_while(|entry| entry.kind == "user")
        {
            let origin = &entry.content["_meta"]["aow/message"]["source"];
            if origin.is_object() {
                return Ok(serde_json::from_value(origin.clone())?);
            }
        }
    }
    source(state, task, thread).await
}

pub(crate) fn new_message(
    task: &TaskRecord,
    request_id: String,
    prompt: String,
    source: Option<MessageSource>,
) -> MessageDelivery {
    MessageDelivery {
        id: uuid::Uuid::new_v4().to_string(),
        request_id,
        task_id: task.id.clone(),
        thread_id: task.initial_thread_id.clone().expect("ACP task"),
        prompt,
        source,
        event_id: None,
        state: DeliveryState::Pending,
        created_at: now(),
        attempts: 0,
        retry_at: 0,
        error: None,
    }
}

pub(super) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
