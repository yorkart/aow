use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

async fn fixture() -> (tempfile::TempDir, AppState, Project, Vec<String>) {
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
    let mut state = AppState::new(PathBuf::new());
    state.aow = AowManager::persistent(&root.join("state")).unwrap();
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(root.join("notes").to_string_lossy().into_owned()),
        })
        .await
        .unwrap();
    let mut paths = Vec::new();
    for branch in ["dirty", "clean", "locked"] {
        let path = root.join(branch).to_string_lossy().into_owned();
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
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
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
                    serde_json::json!({"workspace_root": paths[1], "cwd": paths[1]}).to_string(),
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
