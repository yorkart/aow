use super::*;
use std::time::{Duration, Instant};

pub(super) const STEP_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const STEP_TITLES: [&str; 5] = [
    "等待仓库可用",
    "检查分支和目标路径",
    "更新主仓库（git pull）",
    "创建 Worktree",
    "刷新项目列表",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Pending,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Interrupted,
    Skipped,
}

impl Status {
    pub(super) fn active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::aow) struct CreationStep {
    pub(super) title: String,
    pub(super) status: Status,
    pub(super) started_at: Option<String>,
    pub(super) duration_ms: Option<u64>,
    pub(super) timeout_ms: u64,
    pub(super) message: Option<String>,
    #[serde(skip)]
    pub(super) started: Option<Instant>,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::aow) struct CreationJob {
    pub(super) id: String,
    pub(super) project_id: String,
    pub(super) branch: String,
    pub(super) base_ref: String,
    pub(super) path: String,
    pub(super) pull_first: bool,
    pub(super) status: Status,
    pub(super) steps: Vec<CreationStep>,
    pub(super) error: Option<String>,
    #[serde(skip)]
    pub(super) finished: Option<Instant>,
}

impl CreationJob {
    pub(super) fn new(project_id: String, request: &CreateWorktreeRequest) -> Self {
        Self {
            id: aow_id::new_id(),
            project_id,
            branch: request.branch.clone(),
            base_ref: request.base_ref.clone(),
            path: request.path.clone(),
            pull_first: request.pull_first,
            status: Status::Pending,
            steps: STEP_TITLES
                .iter()
                .map(|title| CreationStep {
                    title: (*title).into(),
                    status: Status::Pending,
                    started_at: None,
                    duration_ms: None,
                    timeout_ms: STEP_TIMEOUT.as_millis() as u64,
                    message: None,
                    started: None,
                })
                .collect(),
            error: None,
            finished: None,
        }
    }
}

#[derive(Default)]
pub(in crate::aow) struct CreationJobs(Mutex<Vec<CreationJob>>);

impl CreationJobs {
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, Vec<CreationJob>>, AowError> {
        let mut jobs = self.0.lock().map_err(|_| AowError::Poisoned)?;
        jobs.retain(|job| {
            job.finished
                .is_none_or(|at| at.elapsed() < Duration::from_secs(300))
        });
        while jobs.iter().filter(|job| job.finished.is_some()).count() > 50 {
            let index = jobs
                .iter()
                .enumerate()
                .filter_map(|(i, job)| job.finished.map(|at| (i, at)))
                .min_by_key(|(_, at)| *at)
                .unwrap()
                .0;
            jobs.remove(index);
        }
        Ok(jobs)
    }

    pub(in crate::aow) fn project_busy(&self, id: &str) -> Result<bool, AowError> {
        Ok(self
            .lock()?
            .iter()
            .any(|job| job.project_id == id && job.status.active()))
    }
}
