use super::now;
use crate::{acp::Controls, aow::AowManager, workspace_events::WorkspaceEvents};
use anyhow::{Context, Result, ensure};
use aow_acp::{AcpService, Content, SessionSnapshot};
use aow_db::tasks::{DeliveryState, MessageDelivery, TaskStatus};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Notify, task::JoinSet};

#[derive(Clone)]
pub(super) struct Worker {
    pub manager: AowManager,
    pub acp: AcpService,
    pub controls: Controls,
    pub events: WorkspaceEvents,
}

pub(super) async fn run(worker: Worker, notify: Arc<Notify>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut jobs = JoinSet::new();
    let mut active = HashSet::new();
    let mut workspaces = HashSet::new();
    let mut identities = HashMap::new();
    let mut changes = worker.acp.subscribe_sync();
    let mut cursor = None;
    loop {
        tokio::select! {
            _ = interval.tick() => {},
            _ = notify.notified() => {},
            event = changes.recv() => {
                if event.is_ok_and(|event| !event.deleted && event.session.is_none_or(|session| session.info.status == "working")) { continue; }
            },
            Some(result) = jobs.join_next_with_id(), if !jobs.is_empty() => {
                let id = match result { Ok((id, ())) => id, Err(error) => { tracing::warn!(%error,"Message delivery stopped; retained for retry"); error.id() } };
                if let Some((thread,workspace)) = identities.remove(&id) {
                    active.remove(&thread);
                    if let Some(workspace) = workspace { workspaces.remove(&workspace); }
                }
                continue;
            }
        }
        let mut pending = match worker.manager.task_db().pending_messages() {
            Ok(pending) => pending,
            Err(error) => {
                tracing::error!(%error,"Cannot read message queue");
                continue;
            }
        };
        // Keep the head of each conversation before rotating: preserve FIFO and
        // prevent busy conversations from starving other targets.
        let mut scheduled = HashSet::new();
        pending.retain(|message| scheduled.insert(message.thread_id.clone()));
        if let Some(index) = pending
            .iter()
            .position(|message| Some(&message.thread_id) == cursor.as_ref())
        {
            let offset = (index + 1) % pending.len();
            pending.rotate_left(offset);
        }
        for message in pending {
            if active.contains(&message.thread_id) || message.retry_at > now() {
                continue;
            }
            if jobs.len() >= 8 {
                break;
            }
            let workspace = worker
                .manager
                .task_policy_for_thread(&worker.acp, &message.thread_id)
                .ok()
                .flatten()
                .filter(|policy| policy.workspace_intent.as_deref() != Some("analyze"))
                .map(|policy| policy.workspace);
            if workspace
                .as_ref()
                .is_some_and(|path| workspaces.contains(path))
            {
                continue;
            }
            let thread = message.thread_id.clone();
            cursor = Some(thread.clone());
            active.insert(thread.clone());
            if let Some(path) = &workspace {
                workspaces.insert(path.clone());
            }
            let worker = worker.clone();
            let handle = jobs.spawn(async move {
                if let Err(error) = worker.deliver(message).await {
                    tracing::warn!(%error,"Cannot deliver message; retained for retry");
                }
            });
            identities.insert(handle.id(), (thread, workspace));
        }
    }
}

fn busy(snapshot: &SessionSnapshot) -> bool {
    snapshot.status == "working"
        || snapshot.steering_pending
        || snapshot.steering_untracked
        || !snapshot.permissions.is_empty()
        || snapshot
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.phase != "failed")
}

impl Worker {
    async fn submit(&self, thread: &str, id: &str, content: Vec<Content>) -> Result<bool> {
        let _lifecycle = self.controls.background(thread).await;
        let snapshot = self.acp.snapshot(thread)?;
        if snapshot.prompt_receipts.contains_key(id) {
            return Ok(true);
        }
        if busy(&snapshot) {
            return Ok(false);
        }
        if let Some(execution) = self.manager.task_policy_for_thread(&self.acp, thread)?
            && execution.workspace_intent.as_deref() != Some("analyze")
            && self
                .acp
                .sessions(Some(&execution.workspace))
                .iter()
                .any(|other| {
                    other.thread_id != thread
                        && self
                            .acp
                            .snapshot(&other.thread_id)
                            .is_ok_and(|snapshot| busy(&snapshot))
                })
        {
            return Ok(false);
        }
        // resume retains the local/native identity. Its failure is surfaced; never
        // replace a missing conversation with an empty one.
        tokio::time::timeout(Duration::from_secs(120), self.acp.resume(thread))
            .await
            .context("Conversation recovery timed out")??;
        if busy(&self.acp.snapshot(thread)?) {
            return Ok(false);
        }
        // Recovery can await the provider while the user changes task state.
        let target = self
            .manager
            .task_for_thread(&self.acp, thread)?
            .context("Task was removed")?;
        ensure!(
            !target.archived && target.status != TaskStatus::Cancelled,
            "Task was archived or cancelled"
        );
        let event = content.iter().any(|block| {
            block
                .data
                .get("_meta")
                .is_some_and(|meta| meta["aow/event"].is_object())
        });
        ensure!(
            event || !target.status.is_closed(),
            "Reopen the task before sending work"
        );
        let content = self.manager.acp_role_content(&self.acp, thread, content)?;
        match self.acp.start_prompt_once(thread, id, content) {
            Ok(()) => Ok(true),
            Err(_)
                if self
                    .acp
                    .snapshot(thread)
                    .is_ok_and(|snapshot| busy(&snapshot)) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    fn active_task(&self, task: &str, thread: &str) -> Result<()> {
        ensure!(
            self.manager
                .task_for_thread(&self.acp, thread)?
                .is_some_and(|item| item.id == task
                    && !item.archived
                    && item.status != TaskStatus::Cancelled),
            "Source or target task was removed, archived, cancelled, or no longer owns this conversation"
        );
        Ok(())
    }

    pub(super) async fn deliver(&self, message: MessageDelivery) -> Result<()> {
        let db = self.manager.task_db();
        // A receipt acknowledges admission. Never mirror or interpret its outcome.
        if self
            .acp
            .snapshot(&message.thread_id)
            .is_ok_and(|snapshot| snapshot.prompt_receipts.contains_key(&message.id))
        {
            db.update_message(&message.id, DeliveryState::Delivered, None, 0)?;
            if message.event_id.is_some() {
                self.events.inbox_changed();
            }
            return Ok(());
        }
        if message.retry_at > now() {
            return Ok(());
        }
        let result = async {
            self.active_task(&message.task_id, &message.thread_id)?;
            if message.event_id.is_none() {
                ensure!(!db.status(&message.task_id)?.is_some_and(TaskStatus::is_closed), "Reopen the task before sending work");
            }
            let content = if let Some(id) = &message.event_id {
                let event = db.event(id)?.context("Inbox event is unavailable")?;
                vec![serde_json::from_value(json!({"type":"text", "text":format!("[WAKEUP]\n{}", serde_json::to_string(&event)?),
                    "_meta":{"aow/event":{"id":event.id,"task_id":event.task_id,"thread_id":event.thread_id,"type":event.kind,"message":event.message}}}))?]
            } else {
                vec![serde_json::from_value(json!({"type":"text", "text":format!("AoW message context (data): {}",json!({"message_id":message.id,"source":message.source})),"_meta":{"aow/instructions":true,"aow/message":{"source":message.source}}}))?,
                     serde_json::from_value(json!({"type":"text","text":message.prompt}))?]
            };
            self.submit(&message.thread_id,&message.id,content).await
        }.await;
        match result {
            Ok(true) => {
                db.update_message(&message.id, DeliveryState::Delivered, None, 0)?;
                if message.event_id.is_some() {
                    self.events.inbox_changed();
                }
            }
            Ok(false) => {}
            Err(error) => {
                let state = if message.attempts < 2 {
                    DeliveryState::Pending
                } else {
                    DeliveryState::Blocked
                };
                db.update_message(
                    &message.id,
                    state,
                    Some(format!("{error:#}")),
                    now() + 5_000 * i64::from(message.attempts + 1),
                )?;
                if message.event_id.is_some() {
                    self.events.inbox_changed();
                }
            }
        }
        Ok(())
    }
}
