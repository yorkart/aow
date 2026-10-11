use super::*;
use aow_operation_log::Outcome;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tower::ServiceExt;

async fn fixture() -> (tempfile::TempDir, AppState, Project, CreateWorktreeRequest) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = root.join("repo");
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
    let state = AppState::new(PathBuf::new());
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(root.join("notes").to_string_lossy().into_owned()),
        })
        .await
        .unwrap();
    let request = request(&root);
    (directory, state, project, request)
}

fn request(root: &Path) -> CreateWorktreeRequest {
    CreateWorktreeRequest {
        branch: "feature/new".into(),
        base_ref: "main".into(),
        path: root.join("repo-new").to_string_lossy().into_owned(),
        pull_first: false,
    }
}

async fn finished(state: &AppState, id: &str) -> CreationJob {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = state
                .aow
                .inner
                .creations
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.id == id)
                .unwrap()
                .clone();
            if !job.status.active() {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn accepts_without_waiting_rejects_duplicates_and_completes_after_request_is_gone() {
    let (directory, state, project, input) = fixture().await;
    let lock = state.aow.inner.removals.project_lock(&project.id).unwrap();
    let guard = lock.lock().await;
    let router = crate::build_router(state.clone());
    let response = tokio::time::timeout(Duration::from_secs(1), router.clone().oneshot(
        Request::post(format!("/api/aow/projects/{}/worktrees", project.id)).header("content-type", "application/json")
            .body(Body::from(serde_json::json!({"branch":input.branch,"base_ref":input.base_ref,"path":input.path,"pull_first":false}).to_string())).unwrap(),
    )).await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    let id = body["id"].as_str().unwrap();
    drop(router); // The HTTP request/router does not own the worker.
    assert!(!Path::new(&input.path).exists());
    assert!(matches!(
        submit(
            state.clone(),
            project.id.clone(),
            request(&directory.path().canonicalize().unwrap())
        ),
        Err(AowError::CreationConflict(_))
    ));
    assert!(matches!(
        state.aow.remove_project(&project.id).await,
        Err(AowError::CreationConflict(_))
    ));
    drop(guard);
    let job = finished(&state, id).await;
    assert_eq!(job.status, Status::Succeeded);
    assert_eq!(
        job.steps.iter().map(|step| step.status).collect::<Vec<_>>(),
        [
            Status::Succeeded,
            Status::Succeeded,
            Status::Skipped,
            Status::Succeeded,
            Status::Succeeded
        ]
    );
    assert!(
        job.steps
            .iter()
            .filter(|step| step.status == Status::Succeeded)
            .all(|step| step.started_at.is_some() && step.duration_ms.is_some())
    );
    assert!(Path::new(&job.path).is_dir());
    let snapshot = state.operations.snapshot();
    let operation = snapshot
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .unwrap();
    assert_eq!(operation.outcome, Some(Outcome::Succeeded));
    assert_eq!(operation.completed, 5);
    let response = crate::build_router(state.clone())
        .oneshot(
            Request::get("/api/aow/worktree-creations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let jobs: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(jobs[0]["steps"][2]["status"], "skipped");
}

#[tokio::test]
async fn waiting_for_the_project_lock_times_out_and_does_not_create_later() {
    let (directory, state, project, input) = fixture().await;
    let lock = state.aow.inner.removals.project_lock(&project.id).unwrap();
    let guard = lock.lock().await;
    let job = api::submit_with_timeout(
        state.clone(),
        project.id.clone(),
        input,
        Duration::from_millis(100),
    )
    .unwrap();
    let failed = finished(&state, &job.id).await;
    assert_eq!(failed.status, Status::TimedOut);
    assert_eq!(failed.steps[0].status, Status::TimedOut);
    assert!(
        failed.steps[1..]
            .iter()
            .all(|step| step.status == Status::Skipped)
    );
    assert!(!state.aow.inner.creations.project_busy(&project.id).unwrap());
    drop(guard);
    assert!(!Path::new(&failed.path).exists());
    let retry = submit(
        state.clone(),
        project.id,
        request(&directory.path().canonicalize().unwrap()),
    )
    .unwrap();
    assert_eq!(finished(&state, &retry.id).await.status, Status::Succeeded);
}

fn hanging_script(path: &Path, marker: &Path) {
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf '%s\\n' \"$$\" \"$child\" > '{}'\nwait\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

async fn helpers_stopped(marker: &Path) {
    let pids: Vec<i32> = std::fs::read_to_string(marker)
        .unwrap()
        .lines()
        .map(|pid| pid.parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if pids.iter().all(|pid| match aow_process::info(*pid) {
                Ok(process) => process.state == 'Z',
                Err(error) => {
                    error.kind() == std::io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(libc::ESRCH)
                }
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Git helper survived step timeout");
}

#[tokio::test]
async fn pull_timeout_kills_helpers_skips_creation_and_allows_retry_without_pull() {
    let (directory, state, project, mut input) = fixture().await;
    let root = directory.path().canonicalize().unwrap();
    let cwd = Path::new(&project.registered_path);
    let script = root.join("upload-pack");
    let marker = root.join("pids");
    hanging_script(&script, &marker);
    git_output(cwd, &["remote", "add", "origin", cwd.to_str().unwrap()])
        .await
        .unwrap();
    git_output(
        cwd,
        &[
            "config",
            "remote.origin.uploadpack",
            script.to_str().unwrap(),
        ],
    )
    .await
    .unwrap();
    git_output(cwd, &["config", "branch.main.remote", "origin"])
        .await
        .unwrap();
    git_output(cwd, &["config", "branch.main.merge", "refs/heads/main"])
        .await
        .unwrap();
    input.pull_first = true;
    let job = api::submit_with_timeout(
        state.clone(),
        project.id.clone(),
        input,
        Duration::from_secs(2),
    )
    .unwrap();
    let failed = finished(&state, &job.id).await;
    assert_eq!(failed.status, Status::TimedOut, "{failed:?}");
    assert_eq!(failed.steps[2].status, Status::TimedOut);
    assert_eq!(failed.steps[3].status, Status::Skipped);
    assert!(!Path::new(&failed.path).exists());
    helpers_stopped(&marker).await;
    assert!(
        git_output(cwd, &["branch", "--list", "feature/new"])
            .await
            .unwrap()
            .is_empty()
    );
    let retry = submit(state.clone(), project.id, request(&root)).unwrap();
    assert_eq!(finished(&state, &retry.id).await.status, Status::Succeeded);
}

#[tokio::test]
async fn checkout_timeout_reports_partial_resources_and_skips_refresh() {
    let (directory, state, project, input) = fixture().await;
    let marker = directory.path().canonicalize().unwrap().join("pids");
    let hook = Path::new(&project.registered_path).join(".git/hooks/post-checkout");
    hanging_script(&hook, &marker);
    let job =
        api::submit_with_timeout(state.clone(), project.id, input, Duration::from_secs(2)).unwrap();
    let failed = finished(&state, &job.id).await;
    assert_eq!(failed.steps[3].status, Status::TimedOut);
    assert_eq!(failed.steps[4].status, Status::Skipped);
    assert!(failed.error.unwrap().contains("分支或目录可能已创建"));
    assert!(Path::new(&failed.path).is_dir());
    helpers_stopped(&marker).await;
}
