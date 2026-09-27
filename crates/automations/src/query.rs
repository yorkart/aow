//! Shared local queries. Opening this facade does not initialize state or start
//! scheduler, cleanup, or agent services. Mutation remains available on Store.
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{Run, Schedule, Store, Task, TaskInput};

#[derive(Debug, Serialize)]
pub struct TaskState {
    /// Calculated from the saved schedule, not an OS scheduler acknowledgement.
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run: Option<Run>,
    pub is_running: bool,
}

/// Shared Web/list representation. Full launch configuration is in TaskDetail.
#[derive(Debug, Serialize)]
pub struct TaskView {
    pub id: String,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub input: TaskInput,
    pub project_name: String,
    pub scheduler_error: Option<String>,
    pub deleted: bool,
    #[serde(flatten)]
    pub state: TaskState,
}

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: Task,
    #[serde(flatten)]
    pub state: TaskState,
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    #[serde(flatten)]
    pub run: Run,
    /// The configuration captured by this execution, not the current task.
    pub configuration: TaskInput,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct RunPage {
    pub items: Vec<RunDetail>,
    pub next_cursor: Option<String>,
}

impl Store {
    fn task_state(&self, task: &Task) -> Result<TaskState> {
        let next_run_at = if !task.deleted
            && task.input.kind == crate::TaskKind::Scheduled
            && task.input.enabled
            && task.scheduler_error.is_none()
            && task.input.interval_seconds.is_none()
        {
            Schedule::parse(&task.input.cron)?.next_after(Utc::now())
        } else {
            None
        };
        Ok(TaskState {
            next_run_at,
            last_run: self.runs(&task.id, None, 1)?.into_iter().next(),
            is_running: self.is_running(&task.id)?,
        })
    }

    pub fn task_view(&self, task: Task) -> Result<TaskView> {
        let state = self.task_state(&task)?;
        Ok(TaskView {
            id: task.id,
            revision: task.revision,
            created_at: task.created_at,
            updated_at: task.updated_at,
            input: task.input,
            project_name: task.project_name,
            scheduler_error: task.scheduler_error,
            deleted: task.deleted,
            state,
        })
    }
}

pub struct AutomationQuery {
    store: Store,
}

impl AutomationQuery {
    pub fn open(state_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            store: Store::open(state_dir)?,
        })
    }

    pub fn tasks(&self, project_id: Option<&str>, include_deleted: bool) -> Result<Vec<TaskView>> {
        self.store
            .all_tasks()?
            .into_iter()
            .filter(|task| include_deleted || !task.deleted)
            .filter(|task| project_id.is_none_or(|id| task.input.project_id == id))
            .map(|task| self.store.task_view(task))
            .collect()
    }

    pub fn task(&self, id: &str) -> Result<TaskDetail> {
        let task = self.store.get_task(id)?;
        let state = self.store.task_state(&task)?;
        Ok(TaskDetail { task, state })
    }

    pub fn run(&self, task_id: &str, run_id: &str) -> Result<RunDetail> {
        self.store.get_task(task_id)?;
        self.store
            .read_run_detail(task_id, run_id)?
            .context("执行记录缺少完整的开始事件")
    }

    pub fn runs(&self, task_id: &str, before: Option<&str>, limit: usize) -> Result<RunPage> {
        anyhow::ensure!(
            (1..=500).contains(&limit),
            "limit must be between 1 and 500"
        );
        if let Some(before) = before {
            crate::store::valid_component(before)?;
        }
        self.store.get_task(task_id)?;
        let mut items = Vec::new();
        for id in self.store.run_ids(task_id, before)? {
            match self.store.read_run_detail(task_id, &id) {
                Ok(Some(run)) => items.push(run),
                Ok(None) => continue,
                // Cleanup can remove a completed run after directory enumeration.
                Err(error) if crate::store::is_not_found(&error) => continue,
                Err(error) => return Err(error),
            }
            if items.len() > limit {
                break;
            }
        }
        let next_cursor = if items.len() > limit {
            items.pop();
            items.last().map(|detail| detail.run.id.clone())
        } else {
            None
        };
        Ok(RunPage { items, next_cursor })
    }
}
