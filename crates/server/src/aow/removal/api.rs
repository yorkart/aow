use super::*;
use crate::operations::Spec;
use std::collections::HashSet;

pub(in crate::aow) async fn list_jobs(
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

pub(in crate::aow) async fn submit_batch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<BatchRequest>,
) -> Result<(StatusCode, Json<RemovalJob>), Response> {
    submit(state, id, request.items)
        .map(|job| (StatusCode::ACCEPTED, Json(job)))
        .map_err(aow_response)
}

pub(in crate::aow) fn submit(
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
        id: aow_id::new_id(),
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
    worker::spawn(state, job.clone(), operation, project_lock);
    Ok(job)
}
