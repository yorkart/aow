use super::now;
use crate::acp::Controls;
use crate::tasks::TaskService;
use crate::workspace_events::WorkspaceEvents;
use anyhow::{Context, Result, ensure};
use aow_db::tasks::{DeliveryState, MessageDelivery, TaskRecord, TaskStatus};
use aow_host_client::acp::AcpClient;
use aow_host_client::acp::{Content, SessionSnapshot};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Notify, task::JoinSet};

#[derive(Clone)]
pub(super) struct Worker {
    pub tasks: TaskService,
    pub acp: AcpClient,
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
    let mut changes = worker.acp.subscribe_sync().await.ok();
    let mut cursor = None;
    loop {
        tokio::select! {
            _ = interval.tick() => {},
            _ = notify.notified() => {},
            event = async { changes.as_mut().expect("subscription checked").recv().await }, if changes.is_some() => {
                match event {
                    Ok(event) if !event.deleted && event.session.as_ref().is_none_or(|session| session.info.status == "working") => continue,
                    Ok(_) => {},
                    Err(_) => changes = None,
                }
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
        if changes.is_none() {
            changes = worker.acp.subscribe_sync().await.ok();
        }
        let mut pending = match worker.tasks.pending_messages() {
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
                .tasks
                .task_policy_for_thread(&worker.acp, &message.thread_id)
                .await
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
        let snapshot = self.acp.snapshot(thread).await?;
        if snapshot.prompt_receipts.contains_key(id) {
            return Ok(true);
        }
        if busy(&snapshot) {
            return Ok(false);
        }
        if let Some(execution) = self.tasks.task_policy_for_thread(&self.acp, thread).await?
            && execution.workspace_intent.as_deref() != Some("analyze")
        {
            for other in self.acp.sessions(Some(&execution.workspace)).await? {
                if other.thread_id != thread && busy(&self.acp.snapshot(&other.thread_id).await?) {
                    return Ok(false);
                }
            }
        }
        // resume retains the local/native identity. Its failure is surfaced; never
        // replace a missing conversation with an empty one.
        tokio::time::timeout(Duration::from_secs(120), self.acp.resume(thread))
            .await
            .context("Conversation recovery timed out")??;
        if busy(&self.acp.snapshot(thread).await?) {
            return Ok(false);
        }
        // Recovery may await the provider. Serialize final policy validation and
        // remote admission with task cancellation so a cancel cannot overtake the RPC.
        let _task_policy = self.tasks.task_resource_access().await;
        let target = self
            .tasks
            .task_for_thread(&self.acp, thread)
            .await?
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
        let content = self
            .tasks
            .acp_role_content(&self.acp, thread, content)
            .await?;
        match self.acp.start_prompt_once(thread, id, content).await {
            Ok(()) => Ok(true),
            Err(_)
                if self
                    .acp
                    .snapshot(thread)
                    .await
                    .is_ok_and(|snapshot| busy(&snapshot)) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    async fn active_task(&self, task: &str, thread: &str) -> Result<TaskRecord> {
        self.tasks.task_for_thread(&self.acp, thread).await?
            .filter(|item| item.id == task && !item.archived && item.status != TaskStatus::Cancelled)
            .context("Source or target task was removed, archived, cancelled, or no longer owns this conversation")
    }

    pub(super) async fn deliver(&self, message: MessageDelivery) -> Result<()> {
        let db = &self.tasks;
        // A receipt acknowledges admission. Never mirror or interpret its outcome.
        if self
            .acp
            .snapshot(&message.thread_id)
            .await
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
            let target = self.active_task(&message.task_id, &message.thread_id).await?;
            if message.event_id.is_none() {
                ensure!(!db.status(&message.task_id)?.is_some_and(TaskStatus::is_closed), "Reopen the task before sending work");
            }
            let content = if let Some(id) = &message.event_id {
                let event = db.event(id)?.context("Inbox event is unavailable")?;
                vec![serde_json::from_value(json!({"type":"text", "text":format!("[WAKEUP]\n{}", serde_json::to_string(&event)?),
                    "_meta":{"aow/event":{"id":event.id,"task_id":event.task_id,"thread_id":event.thread_id,"type":event.kind,"message":event.message}}}))?]
            } else if crate::tasks::is_router_thread(&target, &message.thread_id) {
                vec![serde_json::from_value(json!({"type":"text", "text":format!("AoW message context (data): {}",json!({"message_id":message.id,"source":message.source})),"_meta":{"aow/instructions":true,"aow/message":{"source":message.source}}}))?,
                     serde_json::from_value(json!({"type":"text","text":message.prompt}))?]
            } else {
                vec![serde_json::from_value(json!({"type":"text","text":message.prompt}))?]
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
                let state = if aow_host_client::acp::is_unavailable(&error) || message.attempts < 2
                {
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
