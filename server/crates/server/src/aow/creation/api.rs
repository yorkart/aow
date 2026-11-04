use super::*;
use crate::operations::Spec;

#[expect(
    clippy::result_large_err,
    reason = "Creation route returns an Axum error response directly, like the other AoW routes"
)]
pub(in crate::aow) async fn list_jobs(
    State(state): State<AppState>,
) -> Result<Json<Vec<CreationJob>>, Response> {
    state
        .aow
        .inner
        .creations
        .lock()
        .map(|jobs| Json(jobs.clone()))
        .map_err(aow_response)
}

pub(in crate::aow) fn submit(
    state: AppState,
    project_id: String,
    request: CreateWorktreeRequest,
) -> Result<CreationJob, AowError> {
    submit_with_timeout(state, project_id, request, model::STEP_TIMEOUT)
}

pub(super) fn submit_with_timeout(
    state: AppState,
    project_id: String,
    mut request: CreateWorktreeRequest,
    timeout: std::time::Duration,
) -> Result<CreationJob, AowError> {
    workflow::validate_request(&mut request)?;
    let mut job = CreationJob::new(project_id, &request);
    for step in &mut job.steps {
        step.timeout_ms = timeout.as_millis() as u64;
    }
    {
        // Reserve before spawning, using the same registry-first order as removal.
        // No filesystem or Git work may delay accepting the request.
        let registry = state.aow.lock()?;
        if !registry
            .projects
            .iter()
            .any(|project| project.id == job.project_id)
        {
            return Err(AowError::ProjectNotFound(job.project_id.clone()));
        }
        let mut jobs = state.aow.inner.creations.lock()?;
        if jobs.iter().any(|existing| {
            existing.status.active()
                && (existing.path == job.path
                    || (existing.project_id == job.project_id && existing.branch == job.branch))
        }) {
            return Err(AowError::CreationConflict(
                aow_i18n::text("server.this_branch_or_path_is_already_in_the_creation").into(),
            ));
        }
        jobs.push(job.clone());
    }
    let operation = state.operations.begin(Spec {
        id: job.id.clone(),
        kind: "worktree.create",
        source: "web",
        title: aow_i18n::message(
            "server.create_worktree",
            &[("auto0", job.branch.to_string())],
        ),
        project_id: Some(job.project_id.clone()),
        resource: Some(job.path.clone()),
        total: Some(job.steps.len()),
    });
    let progress = Arc::new(Progress {
        manager: state.aow.clone(),
        id: job.id.clone(),
        operation,
        timeout,
    });
    let id = job.project_id.clone();
    // The worker owns execution independently of the submitting HTTP connection.
    aow_i18n::spawn(async move {
        let observer = progress.clone();
        let worker = aow_i18n::spawn(async move {
            state
                .aow
                .create_worktree_steps(&id, request, Some(&progress))
                .await
        });
        match worker.await {
            Ok(Ok(result)) => observer.finish(
                Status::Succeeded,
                aow_i18n::message(
                    "server.worktree_created",
                    &[("auto0", result.worktree.path.to_string())],
                ),
                Some(result.worktree.path),
            ),
            Ok(Err(error)) => observer.finish(
                if matches!(error, AowError::Timeout(_)) {
                    Status::TimedOut
                } else {
                    Status::Failed
                },
                error.to_string(),
                None,
            ),
            Err(error) => observer.finish(
                Status::Interrupted,
                aow_i18n::message(
                    "server.creation_task_interrupted_refresh_projects_and_check_branch_and",
                    &[("p0", format!("{}", error))],
                ),
                None,
            ),
        }
    });
    Ok(job)
}
