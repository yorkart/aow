use super::*;
use crate::operations::{Handle, Spec};
use aow_operation_log::{Level, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RemovalStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl RemovalStatus {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RemovalRequest {
    pub path: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct BatchRequest {
    items: Vec<RemovalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RemovalItem {
    path: String,
    force: bool,
    status: RemovalStatus,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RemovalJob {
    id: String,
    project_id: String,
    created_at: String,
    items: Vec<RemovalItem>,
    #[serde(skip)]
    finished_at: Option<std::time::Instant>,
}

pub(super) struct RemovalJobs {
    jobs: Mutex<Vec<RemovalJob>>,
    project_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    permits: tokio::sync::Semaphore,
}

impl RemovalJobs {
    pub fn in_memory() -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
            project_locks: Mutex::new(HashMap::new()),
            permits: tokio::sync::Semaphore::new(2),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Vec<RemovalJob>>, AowError> {
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

    fn update(
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

pub(super) async fn list_jobs(
    State(state): State<AppState>,
) -> Result<Json<Vec<RemovalJob>>, Response> {
    state
        .aow
        .inner
        .removals
        .lock()
        .map(|jobs| Json(jobs.clone()))
        .map_err(aow_response)
}

pub(super) async fn submit_batch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<BatchRequest>,
) -> Result<(StatusCode, Json<RemovalJob>), Response> {
    submit(state, id, request.items)
        .map(|job| (StatusCode::ACCEPTED, Json(job)))
        .map_err(aow_response)
}

pub(super) fn submit(
    state: AppState,
    project_id: String,
    requests: Vec<RemovalRequest>,
) -> Result<RemovalJob, AowError> {
    if requests.is_empty() || requests.len() > 200 {
        return Err(AowError::Invalid("每次请选择 1～200 个 Worktree".into()));
    }
    let mut paths = HashSet::new();
    for request in &requests {
        let path = Path::new(&request.path);
        if !path.is_absolute()
            || request.path.len() > 4096
            || request.path.contains('\0')
            || path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            || !paths.insert(request.path.clone())
        {
            return Err(AowError::Invalid("Worktree 路径无效或重复".into()));
        }
    }
    let job = RemovalJob {
        id: Uuid::new_v4().to_string(),
        project_id,
        created_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
        items: requests
            .into_iter()
            .map(|item| RemovalItem {
                path: item.path,
                force: item.force,
                status: RemovalStatus::Queued,
                error: None,
            })
            .collect(),
    };
    let project_lock;
    {
        // Same lock order as removing a Project. No Git scan or terminal daemon call
        // belongs in this request: all potentially slow work runs in the worker.
        let registry = state.aow.lock()?;
        if !registry
            .projects
            .iter()
            .any(|project| project.id == job.project_id)
        {
            return Err(AowError::ProjectNotFound(job.project_id.clone()));
        }
        let removals = &state.aow.inner.removals;
        let mut jobs = removals.lock()?;
        if jobs
            .iter()
            .flat_map(|job| &job.items)
            .any(|item| paths.contains(&item.path) && item.status.active())
        {
            return Err(AowError::RemovalConflict(
                "所选 Worktree 已在清理队列中".into(),
            ));
        }
        project_lock = removals.project_lock(&job.project_id)?;
        let mut next = jobs.clone();
        // Bound completed history while retaining every active task.
        let finished = next
            .iter()
            .filter(|job| job.items.iter().all(|item| !item.status.active()))
            .count();
        let mut discard = finished.saturating_sub(99);
        next.retain(|job| {
            if discard > 0 && job.items.iter().all(|item| !item.status.active()) {
                discard -= 1;
                false
            } else {
                true
            }
        });
        next.push(job.clone());
        *jobs = next;
        for item in &job.items {
            if let Err(error) = state.terminals.set_workspace_removing(&item.path, true) {
                tracing::error!(%error, path = %item.path, "failed to reserve workspace for removal");
            }
        }
    }
    let operation = state.operations.begin(Spec {
        id: job.id.clone(),
        kind: "worktree.remove",
        source: "web",
        title: "删除 Worktree".into(),
        project_id: Some(job.project_id.clone()),
        resource: None,
        total: Some(job.items.len()),
    });
    let worker_job = job.clone();
    // Own the task independently of the HTTP request and catch worker panics so
    // no item is left permanently running and terminal creation is unblocked.
    tokio::spawn(async move {
        let worker_state = state.clone();
        let inner_job = worker_job.clone();
        let worker_operation = operation.clone();
        let worker = tokio::spawn(async move {
            let _project = project_lock.lock().await;
            let _permit = worker_state
                .aow
                .inner
                .removals
                .permits
                .acquire()
                .await
                .expect("removal semaphore closed");
            run_job(&worker_state, &inner_job, &worker_operation).await;
        });
        if let Err(error) = worker.await {
            for item in &worker_job.items {
                let active = state
                    .aow
                    .inner
                    .removals
                    .lock()
                    .map(|jobs| {
                        jobs.iter()
                            .find(|job| job.id == worker_job.id)
                            .is_some_and(|job| {
                                job.items.iter().any(|current| {
                                    current.path == item.path && current.status.active()
                                })
                            })
                    })
                    .unwrap_or(false);
                if active {
                    let _ = state.terminals.set_workspace_removing(&item.path, false);
                    let _ = state.aow.inner.removals.update(
                        &worker_job.id,
                        &item.path,
                        RemovalStatus::Interrupted,
                        Some(format!("清理任务中断：{error}")),
                    );
                }
            }
            operation.finish(
                Outcome::Interrupted,
                "清理任务中断，部分结果未确认，请检查 Worktree 状态",
            );
        } else if let Ok(jobs) = state.aow.inner.removals.lock()
            && let Some(job) = jobs.iter().find(|job| job.id == worker_job.id)
        {
            let succeeded = job
                .items
                .iter()
                .filter(|item| item.status == RemovalStatus::Succeeded)
                .count();
            let failed = job.items.len() - succeeded;
            let outcome = if failed == 0 {
                Outcome::Succeeded
            } else if succeeded == 0 {
                Outcome::Failed
            } else {
                Outcome::PartialSuccess
            };
            operation.finish(
                outcome,
                format!("Worktree 清理完成：成功 {succeeded}，失败 {failed}"),
            );
        }
    });
    Ok(job)
}

async fn run_job(state: &AppState, job: &RemovalJob, operation: &Handle) {
    for (index, item) in job.items.iter().enumerate() {
        operation.progress(format!("正在检查 {}", item.path), Some(index));
        let outcome = async {
            state
                .aow
                .inner
                .removals
                .update(&job.id, &item.path, RemovalStatus::Running, None)?;
            // Inspect before touching resources; main/locked/dirty worktrees retain
            // the existing deletion protections. Recheck dirtiness at removal time.
            let inspection = state
                .aow
                .inspect_worktree_removal(&job.project_id, &item.path)
                .await?;
            if inspection.change_count > 0 && !item.force {
                return Err(AowError::DirtyWorktree(inspection.change_count));
            }
            state
                .terminals
                .set_workspace_removing(&item.path, true)
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            operation.progress(format!("清理 Terminal / Agent：{}", item.path), Some(index));
            state
                .terminals
                .delete_workspace(&item.path)
                .await
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            operation.progress(format!("删除 Worktree 目录：{}", item.path), Some(index));
            state
                .aow
                .remove_worktree_files(&job.project_id, &item.path, item.force)
                .await?;
            Ok::<_, AowError>(())
        }
        .await;
        let (status, error) = match outcome {
            Ok(()) => (RemovalStatus::Succeeded, None),
            Err(error) => (RemovalStatus::Failed, Some(error.to_string())),
        };
        operation.event(
            if error.is_some() {
                Level::Error
            } else {
                Level::Info
            },
            match &error {
                Some(error) => format!("删除失败 {}：{error}", item.path),
                None => format!("已删除 {}", item.path),
            },
            Some(index + 1),
        );
        let _ = state.terminals.set_workspace_removing(&item.path, false);
        if let Err(error) = state
            .aow
            .inner
            .removals
            .update(&job.id, &item.path, status, error)
        {
            tracing::error!(%error, job = %job.id, path = %item.path, "failed to update worktree removal outcome");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    async fn fixture() -> (tempfile::TempDir, AppState, Project, Vec<String>) {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        git_output(&repository, &["init", "-q", "-b", "main"])
            .await
            .unwrap();
        git_output(
            &repository,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "initial",
            ],
        )
        .await
        .unwrap();
        let mut state = AppState::new(PathBuf::new());
        state.aow = AowManager::persistent(&directory.path().join("state")).unwrap();
        let project = state
            .aow
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: Some(
                    directory
                        .path()
                        .join("notes")
                        .to_string_lossy()
                        .into_owned(),
                ),
            })
            .await
            .unwrap();
        let mut paths = Vec::new();
        for branch in ["dirty", "clean", "locked"] {
            let path = directory.path().join(branch).to_string_lossy().into_owned();
            state
                .aow
                .create_worktree(
                    &project.id,
                    CreateWorktreeRequest {
                        branch: branch.into(),
                        base_ref: "main".into(),
                        path: path.clone(),
                        pull_first: false,
                    },
                )
                .await
                .unwrap();
            paths.push(path);
        }
        std::fs::write(Path::new(&paths[0]).join("uncommitted.txt"), "keep me").unwrap();
        git_output(&repository, &["worktree", "lock", &paths[2]])
            .await
            .unwrap();
        (directory, state, project, paths)
    }

    async fn wait_finished(state: &AppState, id: &str) -> RemovalJob {
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let job = state
                    .aow
                    .inner
                    .removals
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|job| job.id == id)
                    .unwrap()
                    .clone();
                if job.items.iter().all(|item| !item.status.active()) {
                    return job;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn request_returns_accepted_while_worker_waits_and_rejects_duplicate_deletion() {
        let (_directory, state, project, paths) = fixture().await;
        let manager = state.aow.clone();
        let permit = manager
            .inner
            .removals
            .permits
            .acquire_many(2)
            .await
            .unwrap();
        let router = crate::build_router(state.clone());
        let request = || {
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/aow/projects/{}/worktrees/removals",
                    project.id
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"items": [{"path": paths[1]}]}).to_string(),
                ))
                .unwrap()
        };
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            router.clone().oneshot(request()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let job: RemovalJob =
            serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap())
                .unwrap();
        assert_eq!(job.items[0].status, RemovalStatus::Queued);
        assert!(Path::new(&paths[1]).exists());
        assert_eq!(
            router.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert!(matches!(
            state.aow.remove_project(&project.id).await,
            Err(AowError::RemovalConflict(_))
        ));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/terminals")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"workspace_root": paths[1], "cwd": paths[1]})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.status().is_success());
        let error = String::from_utf8(
            to_bytes(response.into_body(), 100_000)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(error.contains("Worktree 正在清理"), "{error}");
        drop(permit);
        assert_eq!(
            wait_finished(&state, &job.id).await.items[0].status,
            RemovalStatus::Succeeded
        );
        assert!(!Path::new(&paths[1]).exists());
    }

    #[tokio::test]
    async fn batch_isolates_failures_protects_main_locked_and_dirty_and_supports_explicit_retry() {
        let (directory, state, project, paths) = fixture().await;
        let outside = directory.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let job = submit(
            state.clone(),
            project.id.clone(),
            [
                paths[0].clone(),
                paths[1].clone(),
                paths[2].clone(),
                project.registered_path.clone(),
                outside.to_string_lossy().into_owned(),
            ]
            .into_iter()
            .map(|path| RemovalRequest { path, force: false })
            .collect(),
        )
        .unwrap();
        let completed = wait_finished(&state, &job.id).await;
        assert_eq!(
            completed
                .items
                .iter()
                .map(|item| item.status)
                .collect::<Vec<_>>(),
            vec![
                RemovalStatus::Failed,
                RemovalStatus::Succeeded,
                RemovalStatus::Failed,
                RemovalStatus::Failed,
                RemovalStatus::Failed
            ]
        );
        assert!(Path::new(&paths[0]).join("uncommitted.txt").exists());
        assert!(!Path::new(&paths[1]).exists());
        assert!(Path::new(&paths[2]).exists());
        assert!(Path::new(&project.registered_path).exists());
        assert!(outside.exists());
        let retry = submit(
            state.clone(),
            project.id.clone(),
            vec![RemovalRequest {
                path: paths[0].clone(),
                force: true,
            }],
        )
        .unwrap();
        assert_eq!(
            wait_finished(&state, &retry.id).await.items[0].status,
            RemovalStatus::Succeeded
        );
        assert!(!Path::new(&paths[0]).exists());
        let branches = git_output(
            Path::new(&project.registered_path),
            &["branch", "--format=%(refname:short)"],
        )
        .await
        .unwrap();
        assert!(branches.lines().any(|branch| branch == "dirty"));
        assert!(branches.lines().any(|branch| branch == "clean"));
        let restored = AowManager::persistent(&directory.path().join("state")).unwrap();
        assert!(restored.inner.removals.lock().unwrap().is_empty());
        assert!(
            !directory
                .path()
                .join("state/worktree-removals.json")
                .exists()
        );
    }

    #[test]
    fn legacy_history_is_ignored_even_when_invalid() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("worktree-removals.json"), "not json").unwrap();
        let restored = AowManager::persistent(directory.path()).unwrap();
        assert!(restored.inner.removals.lock().unwrap().is_empty());
    }
}
