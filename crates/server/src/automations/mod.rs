use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use aow_automations::{
    AgentLaunch, Run, RunOutput, RunSource, Scheduler, Store, Task, TaskInput, TaskKind, TaskView,
    WorkspaceMode, scheduler::SchedulerStatus,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::Utc;
use serde::Deserialize;

use crate::{AppState, PROCESS_HOME};

mod api;
mod hosting;
mod notifications;
pub(crate) use api::{cli_routes, routes};

const RUN_HISTORY_RETENTION: usize = 200;
const RUN_HISTORY_CLEANUP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FAILURE_NOTIFICATION_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct AutomationManager {
    store: Arc<Store>,
    scheduler: Scheduler,
    operation: Arc<tokio::sync::Mutex<()>>,
}

impl AutomationManager {
    pub(crate) fn new(
        state_dir: PathBuf,
        notifications: crate::notifications::NotificationManager,
    ) -> Result<Self> {
        let manager = Self {
            store: Arc::new(Store::new(state_dir)?),
            scheduler: Scheduler::new(&PROCESS_HOME),
            operation: Arc::new(tokio::sync::Mutex::new(())),
        };
        manager.start_background_tasks(notifications);
        Ok(manager)
    }

    fn start_background_tasks(&self, notifications: crate::notifications::NotificationManager) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let store = Arc::downgrade(&self.store);
        handle.spawn(async move {
            let mut last_cleanup = None;
            let warnings = notifications::Warnings::default();
            let mut interval = tokio::time::interval(FAILURE_NOTIFICATION_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                warnings.flush();
                let Some(store) = store.upgrade() else { break };
                let prune = last_cleanup
                    .is_none_or(|last: Instant| last.elapsed() >= RUN_HISTORY_CLEANUP_INTERVAL);
                let result = notifications::poll(
                    store,
                    |run, channel, delivery_id| {
                        let notifications = notifications.clone();
                        async move {
                            notifications
                                .send_automation_failure(run, channel, &delivery_id)
                                .await
                        }
                    },
                    prune,
                    warnings.clone(),
                )
                .await;
                // Failed cleanup attempts observe the same interval as successful ones.
                if prune {
                    last_cleanup = Some(Instant::now());
                }
                if let Err(error) = result {
                    warnings.report("poll failure notifications", "", "", &error);
                }
            }
        });
    }

    async fn save(&self, mut task: Task) -> Result<TaskView> {
        // Persist the desired configuration first. Even an old installed trigger
        // reads the new enabled flag, and a dropped HTTP request leaves a visible error.
        task.scheduler_error = Some("定时器配置待同步".into());
        self.store.save_task(&task)?;
        task.scheduler_error = self
            .scheduler
            .sync(&self.store, &task)
            .await
            .err()
            .map(|e| format!("{e:#}"));
        self.store.save_task(&task)?;
        self.view(task)
    }

    fn view(&self, task: Task) -> Result<TaskView> {
        self.store.task_view(task)
    }

    fn task(&self, id: &str) -> Result<Task> {
        let task = self.store.get_task(id)?;
        ensure!(!task.deleted, "自动化任务已删除");
        Ok(task)
    }

    pub(crate) fn run(&self, task_id: &str, run_id: &str) -> Result<Run> {
        self.store.get_task(task_id)?;
        self.store
            .read_run(task_id, run_id)?
            .context("执行记录尚未生成")
    }

    pub(crate) fn run_result(&self, run: &Run) -> Result<String> {
        self.store.read_run_result(run)
    }
}

#[cfg(test)]
mod hosting_tests;
#[cfg(test)]
mod tests;
