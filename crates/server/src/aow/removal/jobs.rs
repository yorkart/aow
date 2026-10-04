use super::*;

pub(in crate::aow) struct RemovalJobs {
    pub(super) jobs: Mutex<Vec<RemovalJob>>,
    pub(super) project_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) permits: tokio::sync::Semaphore,
}

impl RemovalJobs {
    pub fn in_memory() -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
            project_locks: Mutex::new(HashMap::new()),
            permits: tokio::sync::Semaphore::new(2),
        }
    }

    pub(super) fn lock(&self) -> Result<MutexGuard<'_, Vec<RemovalJob>>, AowError> {
        let mut jobs = self.jobs.lock().map_err(|_| AowError::Poisoned)?;
        // A short grace period lets clients consume final results. History lives
        // only in the shared operation log and never restores execution state.
        jobs.retain(|job| {
            job.finished_at
                .is_none_or(|at| at.elapsed() < std::time::Duration::from_secs(300))
        });
        Ok(jobs)
    }

    pub fn project_busy(&self, id: &str) -> Result<bool, AowError> {
        Ok(self
            .lock()?
            .iter()
            .any(|job| job.project_id == id && job.items.iter().any(|item| item.status.active())))
    }

    pub fn project_lock(&self, id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, AowError> {
        Ok(self
            .project_locks
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }

    pub(super) fn update(
        &self,
        id: &str,
        path: &str,
        status: RemovalStatus,
        error: Option<String>,
    ) -> Result<(), AowError> {
        let mut jobs = self.lock()?;
        let job = jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| AowError::Invalid("清理任务不存在".into()))?;
        let item = job
            .items
            .iter_mut()
            .find(|item| item.path == path)
            .ok_or_else(|| AowError::Invalid("清理项目不存在".into()))?;
        item.status = status;
        item.error = error;
        if job.items.iter().all(|item| !item.status.active()) {
            job.finished_at = Some(std::time::Instant::now());
        }
        Ok(())
    }
}
