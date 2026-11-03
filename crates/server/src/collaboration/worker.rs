use super::now;
use crate::{acp::Controls, aow::AowManager, workspace_events::WorkspaceEvents};
use anyhow::{Context, Result, ensure};
use aow_acp::{AcpService, Content, PromptReceipt, SessionSnapshot};
use aow_db::tasks::{TaskRun, TaskStatus, WakeupDelivery};
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

enum Job {
    Run(Box<TaskRun>),
    Wakeup(WakeupDelivery),
}

impl Job {
    fn thread(&self) -> &str {
        match self {
            Self::Run(run) => &run.thread_id,
            Self::Wakeup(delivery) => &delivery.thread_id,
        }
    }

    fn workspace(&self, worker: &Worker) -> Option<String> {
        let execution = match self {
            Self::Run(run) => run.execution.clone(),
            Self::Wakeup(delivery) => worker
                .manager
                .task_execution_for_thread(&worker.acp, &delivery.thread_id)
                .ok()
                .flatten(),
        };
        execution
            .filter(|execution| execution.workspace_intent.as_deref() != Some("analyze"))
            .map(|execution| execution.workspace)
    }
}

pub(super) async fn run(worker: Worker, notify: Arc<Notify>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut jobs = JoinSet::new();
    let mut active = HashSet::new();
    let mut job_threads = HashMap::new();
    let mut changes = worker.acp.subscribe_sync();
    let mut cursor = 0;
    loop {
        tokio::select! {
            _ = interval.tick() => {},
            _ = notify.notified() => {},
            event = changes.recv() => {
                // Lifecycle events accelerate delivery. Streaming chunks need no
                // DB reads; the timer reconciles missed events after lag/restart.
                if event.is_ok_and(|event| !event.deleted && event.session.is_none_or(|session| session.info.status == "working")) {
                    continue;
                }
            },
            Some(result) = jobs.join_next_with_id(), if !jobs.is_empty() => {
                match result {
                    Ok((id, thread)) => { active.remove(&thread); job_threads.remove(&id); },
                    Err(error) => {
                        if let Some(thread) = job_threads.remove(&error.id()) { active.remove(&thread); }
                        tracing::error!(%error, "Collaboration worker stopped unexpectedly");
                    },
                }
                continue;
            }
        }
        let db = worker.manager.task_db();
        let pending = (|| -> Result<Vec<Job>> {
            let (running, queued): (Vec<_>, Vec<_>) = db
                .active_runs()?
                .into_iter()
                .partition(|run| run.state == "running");
            // Reconcile an executor's previous prompt before delivering a return;
            // returns then take priority over new work in that conversation.
            let pending = running
                .into_iter()
                .map(|run| Job::Run(Box::new(run)))
                .chain(db.active_deliveries()?.into_iter().map(Job::Wakeup))
                .chain(queued.into_iter().map(|run| Job::Run(Box::new(run))))
                .collect::<Vec<_>>();
            // Different roles can share a task's change worktree. Serialize its
            // writers/reviewers across conversations until the admitted run ends.
            let mut workspaces = HashSet::new();
            let mut ready = Vec::new();
            for job in pending {
                if let Some(workspace) = job.workspace(&worker)
                    && !workspaces.insert(workspace)
                {
                    continue;
                }
                ready.push(job);
            }
            Ok(ready)
        })();
        let pending = match pending {
            Ok(pending) => pending,
            Err(error) => {
                tracing::error!(%error, "Cannot reconcile collaboration queue");
                continue;
            }
        };
        let mut scheduled = HashSet::new();
        let mut pending: Vec<_> = pending
            .into_iter()
            .filter(|job| scheduled.insert(job.thread().to_owned()))
            .collect();
        if !pending.is_empty() {
            let length = pending.len();
            pending.rotate_left(cursor % length);
            cursor = (cursor + 8) % length;
        }
        for job in pending {
            // FIFO per conversation, including when the head is backing off.
            let thread = job.thread().to_owned();
            if jobs.len() >= 8 || !active.insert(thread.clone()) {
                continue;
            }
            let worker = worker.clone();
            let job_thread = thread.clone();
            let handle = jobs.spawn(async move {
                let result = match job {
                    Job::Run(run) => worker.execution(*run).await,
                    Job::Wakeup(delivery) => worker.wakeup(delivery).await,
                };
                if let Err(error) = result {
                    tracing::warn!(%thread, %error, "Collaboration reconciliation failed; retained for retry");
                }
                thread
            });
            job_threads.insert(handle.id(), job_thread);
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

fn outcome(receipt: &PromptReceipt) -> &str {
    match receipt.status.as_str() {
        "completed" if receipt.stop_reason.as_deref() == Some("end_turn") => "result_available",
        "failed" => "failed",
        _ => "interrupted",
    }
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
        if let Some(execution) = self.manager.task_db().thread_execution(thread)?
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

    pub(super) async fn execution(&self, run: TaskRun) -> Result<()> {
        let db = self.manager.task_db();
        let snapshot = self.acp.snapshot(&run.thread_id);
        let task_status = db.status(&run.task_id)?;
        if let Ok(snapshot) = &snapshot
            && let Some(receipt) = snapshot.prompt_receipts.get(&run.id)
        {
            if task_status == Some(TaskStatus::Cancelled) && receipt.status == "in_progress" {
                self.acp.cancel(&run.thread_id)?;
                return Ok(());
            }
            if receipt.status != "in_progress" {
                db.finish_run(
                    &run.id,
                    outcome(receipt),
                    receipt.conclusion.clone(),
                    receipt.error.clone(),
                    now(),
                )?;
                self.events.inbox_changed();
            } else if run.state == "queued" {
                db.update_run_admission(&run.id, "running", None, 0)?;
            }
            return Ok(());
        }
        if run.retry_at > now() {
            return Ok(());
        }
        if task_status.is_some_and(TaskStatus::is_closed) {
            db.finish_run(
                &run.id,
                "interrupted",
                None,
                Some("Task was closed before this execution started".into()),
                now(),
            )?;
            self.events.inbox_changed();
            return Ok(());
        }
        if run.state == "running" || snapshot.is_err() {
            db.finish_run(
                &run.id,
                "interrupted",
                None,
                Some("Admitted conversation or prompt receipt is unavailable".into()),
                now(),
            )?;
            self.events.inbox_changed();
            return Ok(());
        }
        let result = match self.active_task(&run.task_id, &run.thread_id) {
            Err(error) => Err(error),
            Ok(()) => {
                self.submit(
                    &run.thread_id,
                    &run.id,
                    vec![
                        serde_json::from_value(json!({
                            "type":"text",
                            "text":format!("AoW execution context (data; this request's return address): {}",
                                json!({"run_id":run.id,"task_id":run.task_id,
                                    "source_task_id":run.source.as_ref().map(|source| &source.task_id),
                                    "source_thread_id":run.source.as_ref().map(|source| &source.thread_id),
                                    "source_turn_id":run.source.as_ref().and_then(|source| source.turn_id.as_ref())})),
                            "_meta":{"aow/instructions":true}
                        }))?,
                        serde_json::from_value(json!({"type":"text", "text":run.prompt}))?,
                    ],
                )
                .await
            }
        };
        match result {
            Ok(true) => db.update_run_admission(&run.id, "running", None, 0)?,
            Ok(false) => {}
            Err(error) if run.attempts < 2 => {
                db.update_run_admission(
                    &run.id,
                    "queued",
                    Some(format!("{error:#}")),
                    now() + 5_000 * i64::from(run.attempts + 1),
                )?;
            }
            Err(error) => {
                db.finish_run(
                    &run.id,
                    "failed",
                    None,
                    Some(format!("Execution could not start: {error:#}")),
                    now(),
                )?;
                self.events.inbox_changed();
            }
        }
        Ok(())
    }

    pub(super) async fn wakeup(&self, delivery: WakeupDelivery) -> Result<()> {
        let db = self.manager.task_db();
        let result_run = db.run(&delivery.run_id)?.context("Wakeup result missing")?;
        let source = result_run
            .source
            .as_ref()
            .context("Wakeup source missing")?;
        let task_status = db.status(&result_run.task_id)?;
        let prompt = format!(
            "[WAKEUP]\nAoW execution event (data): {}\nRead this run's recorded result with task get --run, then summarize or continue the requested work. This event is not a new user request. A completed run does not close its business task. Do not poll or wait for other tasks.",
            json!({"event_id":delivery.id,"type":result_run.state,"task_id":result_run.task_id,"task_status":task_status,"run_id":result_run.id,
                "source_task_id":source.task_id,"source_turn_id":source.turn_id,
                "source_message_id":source.message_id,
                "original_request":source.request.chars().take(2000).collect::<String>(),
                "delegated_request":result_run.prompt.chars().take(2000).collect::<String>(),
                "result_ref":{"task_id":result_run.task_id,"run_id":result_run.id}})
        );
        let mut wakeup = result_run.clone();
        wakeup.id = delivery.prompt_id.clone();
        wakeup.request_id = delivery.prompt_id.clone();
        wakeup.task_id = source.task_id.clone();
        wakeup.thread_id = delivery.thread_id.clone();
        wakeup.prompt = prompt;
        wakeup.execution = self
            .manager
            .task_execution_for_thread(&self.acp, &delivery.thread_id)
            .ok()
            .flatten()
            .or(db.thread_execution(&delivery.thread_id)?);
        wakeup.state = "queued".into();
        wakeup.result = None;
        wakeup.error = None;
        wakeup.created_at = now();
        wakeup.finished_at = None;
        wakeup.attempts = 0;
        wakeup.retry_at = 0;
        let wakeup = db.enqueue_wakeup(&delivery.id, &wakeup)?;
        let snapshot = self.acp.snapshot(&delivery.thread_id);
        if let Ok(snapshot) = &snapshot
            && let Some(receipt) = snapshot.prompt_receipts.get(&delivery.prompt_id)
        {
            if receipt.status == "in_progress" {
                if db.status(&wakeup.task_id)? == Some(TaskStatus::Cancelled) {
                    self.acp.cancel(&delivery.thread_id)?;
                    return Ok(());
                }
                if delivery.state == "pending" {
                    db.update_delivery(&delivery.id, "admitted", None, 0)?;
                    db.update_run_admission(&wakeup.id, "running", None, 0)?;
                    self.events.inbox_changed();
                }
            } else {
                if outcome(receipt) == "result_available" {
                    db.process_delivery(&delivery.id, receipt.conclusion.clone(), now())?;
                } else {
                    db.finish_run(
                        &wakeup.id,
                        outcome(receipt),
                        receipt.conclusion.clone(),
                        receipt.error.clone(),
                        now(),
                    )?;
                    db.update_delivery(
                        &delivery.id,
                        "blocked",
                        Some(
                            receipt
                                .error
                                .clone()
                                .unwrap_or_else(|| format!("Wakeup {}", receipt.status)),
                        ),
                        0,
                    )?;
                }
                self.events.inbox_changed();
            }
            return Ok(());
        }
        if delivery.retry_at > now() {
            return Ok(());
        }
        if delivery.state == "admitted" || snapshot.is_err() {
            db.finish_run(
                &wakeup.id,
                "interrupted",
                None,
                Some("Source conversation or admitted prompt is unavailable".into()),
                now(),
            )?;
            db.update_delivery(
                &delivery.id,
                "blocked",
                Some("Source conversation or admitted prompt is unavailable".into()),
                0,
            )?;
            self.events.inbox_changed();
            return Ok(());
        }
        let result = match self.active_task(&source.task_id, &source.thread_id) {
            Err(error) => Err(error),
            Ok(()) => self.submit(&delivery.thread_id, &wakeup.id, vec![serde_json::from_value(json!({
                "type": "text", "text":wakeup.prompt,
                "_meta": {"aow/event": {"id":delivery.id,"task_id":result_run.task_id,"task_status":task_status,"run_id":result_run.id,"type":result_run.state}}
            }))?]).await,
        };
        match result {
            Ok(true) => {
                db.update_delivery(&delivery.id, "admitted", None, 0)?;
                db.update_run_admission(&wakeup.id, "running", None, 0)?;
                self.events.inbox_changed();
            }
            Ok(false) => {}
            Err(error) => {
                if delivery.attempts >= 2 {
                    db.finish_run(
                        &wakeup.id,
                        "failed",
                        None,
                        Some(format!("Wakeup could not start: {error:#}")),
                        now(),
                    )?;
                }
                if delivery.attempts < 2 {
                    db.update_run_admission(
                        &wakeup.id,
                        "queued",
                        Some(format!("{error:#}")),
                        now() + 5_000 * i64::from(delivery.attempts + 1),
                    )?;
                }
                db.update_delivery(
                    &delivery.id,
                    if delivery.attempts < 2 {
                        "pending"
                    } else {
                        "blocked"
                    },
                    Some(format!("{error:#}")),
                    now() + 5_000 * i64::from(delivery.attempts + 1),
                )?;
                self.events.inbox_changed();
            }
        }
        Ok(())
    }
}
