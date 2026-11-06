use super::*;
use crate::test_support::{TestServices, git_output};
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
    let mut state = AppState::for_test(PathBuf::new());
    state.replace_services(TestServices::persistent(&root.join("state")).unwrap());
    let project = state
        .projects
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
            .projects
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
                .projects
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

struct TaskHost;

#[async_trait::async_trait]
impl aow_host_client::testing::AcpFixture for TaskHost {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(serde_json::json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/acp/lineage-agent.mjs")
        ]}}}).to_string())
    }

    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
}

#[tokio::test]
async fn queued_removal_does_not_deadlock_task_creation_in_the_same_project() {
    let (directory, mut state, project, paths) = fixture().await;
    state.acp = aow_host_client::acp::AcpClient::for_test_host(
        Arc::new(TaskHost),
        Some(directory.path().join("acp")),
    )
    .unwrap();
    // Keep cleanup queued while a real task creation takes the task lock and
    // creates a worktree in the same project. Cleanup must not own its project
    // lock while waiting for capacity (or later acquire task after project).
    let permits = state
        .projects
        .inner
        .removals
        .permits
        .acquire_many(2)
        .await
        .unwrap();
    let job = submit(
        state.clone(),
        project.id.clone(),
        vec![RemovalRequest {
            path: paths[1].clone(),
            force: false,
        }],
    )
    .await
    .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::tasks::routes().with_state(state.clone()).oneshot(
            Request::post("/api/tasks")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "request_id":"concurrent-task","project_id":project.id,"prompt":"Inspect",
                        "agent_id":"test","workspace_mode":"new_worktree"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ),
    )
    .await;
    drop(permits);
    let response = response
        .expect("queued cleanup must not hold locks needed by task creation")
        .unwrap();
    let code = response.status();
    let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    assert_eq!(code, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let task: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        wait_finished(&state, &job.id).await.items[0].status,
        RemovalStatus::Succeeded
    );
    assert!(!Path::new(&paths[1]).exists());
    assert!(Path::new(task["workspace"].as_str().unwrap()).is_dir());
    let thread = task["initial_thread_id"].as_str().unwrap();
    assert!(state.acp.session_info(thread).await.is_ok());
    state.acp.close_session(thread).await.unwrap();
}

#[tokio::test]
async fn removal_and_task_creation_share_the_same_lock_order() {
    use futures_util::FutureExt;
    let (directory, mut state, project, paths) = fixture().await;
    state.acp = aow_host_client::acp::AcpClient::for_test_host(
        Arc::new(TaskHost),
        Some(directory.path().join("acp")),
    )
    .unwrap();
    let project_lock = state
        .projects
        .inner
        .removals
        .project_lock(&project.id)
        .unwrap();
    let gate = project_lock.lock().await;
    let job = submit(
        state.clone(),
        project.id.clone(),
        vec![RemovalRequest {
            path: paths[1].clone(),
            force: false,
        }],
    )
    .await
    .unwrap();
    // With capacity available, put cleanup first in the project's lock queue.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while state.projects.inner.removals.permits.available_permits() == 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let create = tokio::spawn(
        crate::tasks::routes().with_state(state.clone()).oneshot(
            Request::post("/api/tasks")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "request_id":"lock-order-task","project_id":project.id,"prompt":"Inspect",
                        "agent_id":"test","workspace_mode":"new_worktree"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ),
    );
    // The task lock is now held by cleanup (correct order), or by creation if
    // cleanup regresses to project -> task. Releasing the gate must let both
    // complete, rather than giving cleanup the project lock needed by creation.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while state.tasks.task_resource_access().now_or_never().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(gate);
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), create)
        .await
        .expect("task and project locks must not form a cycle")
        .unwrap()
        .unwrap();
    let code = response.status();
    let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    assert_eq!(code, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        wait_finished(&state, &job.id).await.items[0].status,
        RemovalStatus::Succeeded
    );
    let task: serde_json::Value = serde_json::from_slice(&body).unwrap();
    state
        .acp
        .close_session(task["initial_thread_id"].as_str().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn request_returns_accepted_while_worker_waits_and_rejects_duplicate_deletion() {
    let (_directory, state, project, paths) = fixture().await;
    let manager = state.clone();
    let permit = manager
        .projects
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
            .uri(format!("/api/projects/{}/worktrees/removals", project.id))
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
        state.projects.remove_project(&project.id).await,
        Err(ServiceError::RemovalConflict(_))
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
    assert!(error.contains("Worktree is being cleaned up"), "{error}");
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
    .await
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
    .await
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
    let restored = TestServices::persistent(&directory.path().join("state")).unwrap();
    assert!(restored.projects.inner.removals.lock().unwrap().is_empty());
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
    let restored = TestServices::persistent(directory.path()).unwrap();
    assert!(restored.projects.inner.removals.lock().unwrap().is_empty());
}

#[tokio::test]
async fn removal_cleans_local_acp_sessions_and_preserves_rejected_worktrees() {
    let (_directory, mut state, project, paths) = fixture().await;
    let settings = state.node.acp_settings().unwrap();
    state
        .node
        .save_acp_settings(
            serde_json::json!({"agent_servers":{"test":{"type":"custom","command":"/bin/sh"}}})
                .to_string(),
            settings.revision,
        )
        .await
        .unwrap();
    state.acp = crate::acp::service(
        &state.projects,
        &state.notifications,
        &state.cli_access,
        &state.node,
        None,
        state.terminals.task_stop_sender(),
        state.workspace_events.clone(),
    )
    .unwrap();
    let retained = state
        .acp
        .prepare_session("test", paths[0].clone().into())
        .await
        .unwrap();
    let removed = state
        .acp
        .prepare_session("test", paths[1].clone().into())
        .await
        .unwrap();
    let job = submit(
        state.clone(),
        project.id.clone(),
        vec![
            RemovalRequest {
                path: paths[0].clone(),
                force: false,
            },
            RemovalRequest {
                path: paths[1].clone(),
                force: false,
            },
        ],
    )
    .await
    .unwrap();
    let finished = wait_finished(&state, &job.id).await;
    assert_eq!(finished.items[0].status, RemovalStatus::Failed);
    assert_eq!(finished.items[1].status, RemovalStatus::Succeeded);
    assert!(state.acp.snapshot(&retained.thread_id).await.is_ok());
    assert!(state.acp.snapshot(&removed.thread_id).await.is_err());
    assert!(
        state
            .acp
            .sessions(Some(&paths[1]))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!Path::new(&paths[1]).exists());
}
