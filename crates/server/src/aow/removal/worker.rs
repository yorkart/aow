use super::*;
use crate::operations::Handle;
use aow_operation_log::{Level, Outcome};

pub(super) fn spawn(
    state: AppState,
    worker_job: RemovalJob,
    operation: Handle,
    project_lock: Arc<tokio::sync::Mutex<()>>,
) {
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
}

pub(super) async fn run_job(state: &AppState, job: &RemovalJob, operation: &Handle) {
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
