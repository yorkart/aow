use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

pub(super) async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}
pub(super) fn stored(id: &str, parent: Option<&str>) -> TaskRecord {
    TaskRecord {
        workspace_mode: "existing".into(),
        slug: None,
        id: id.into(),
        project_id: "project".into(),
        parent_id: parent.map(str::to_owned),
        prompt: "Review".into(),
        agent_runtime_id: "test".into(),
        agent_role_id: None,
        session_id: None,
        workspace: "/repo".into(),
        thread_id: format!("thread-{id}"),
        pinned: false,
        archived: false,
    }
}
#[tokio::test]
async fn api_lists_pins_archives_and_deletes_subtrees() {
    let state = AppState::new(Default::default());
    state.aow.inner.tasks.insert(&stored("root", None)).unwrap();
    state
        .aow
        .inner
        .tasks
        .insert(&stored("child", Some("root")))
        .unwrap();
    let app = routes().with_state(state.clone());
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/aow/tasks/child",
            serde_json::json!({"pinned":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, rows) = call(&app, "GET", "/api/aow/tasks", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rows[1]["parent_id"], "root");
    assert_eq!(rows[1]["pinned"], true);
    assert!(state.aow.protect_task_session("thread-root").is_err());
    assert!(state.aow.protect_task_workspace("/repo").is_err());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/aow/tasks/root/archive",
            serde_json::json!({})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        state
            .aow
            .inner
            .tasks
            .list()
            .unwrap()
            .iter()
            .all(|task| task.archived && !task.pinned)
    );
    assert!(state.aow.protect_task_session("thread-root").is_ok());
    assert!(state.aow.protect_task_workspace("/repo").is_ok());
    assert_eq!(
        call(&app, "DELETE", "/api/aow/tasks/root", serde_json::json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(state.aow.inner.tasks.list().unwrap().is_empty());
}
#[tokio::test]
async fn invalid_task_does_not_allocate_metadata() {
    let state = AppState::new(Default::default());
    let app = routes().with_state(state.clone());
    let (status, _) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"project_id":"missing", "prompt":" ", "agent_runtime_id":"missing", "workspace":"/repo"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(state.aow.inner.tasks.list().unwrap().is_empty());
}

struct Host;
#[async_trait::async_trait]
impl aow_acp::AcpHost for Host {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(serde_json::json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")]}}}).to_string())
    }
    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> anyhow::Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> anyhow::Result<()> {
        unreachable!()
    }
}
#[tokio::test]
async fn creation_is_idempotent_and_binds_runtime_lineage_and_protected_session() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut state = AppState::new(Default::default());
    state.acp =
        aow_acp::AcpService::new(Arc::new(Host), Some(directory.path().join("acp"))).unwrap();
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: repo.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let app = routes()
        .merge(crate::acp::routes())
        .with_state(state.clone());
    let request = serde_json::json!({"request_id":"create-root", "project_id":project.id, "agent_runtime_id":"test", "workspace":repo, "prompt":"Inspect this repository"});
    let (status, parent) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{parent}");
    let (_, repeated) = call(&app, "POST", "/api/aow/tasks", request).await;
    assert_eq!(parent, repeated);
    assert_eq!(state.aow.inner.tasks.list().unwrap().len(), 1);
    let thread = parent["thread_id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/acp/sessions/{thread}"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/acp/sessions/{thread}/actions"),
        serde_json::json!({"action":"close"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(state.acp.session_info(thread).is_ok());
    let (status, child) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"create-child", "parent_id":parent["id"], "project_id":project.id, "agent_runtime_id":"test", "workspace":repo, "prompt":"Review the result"})).await;
    assert_eq!(status, StatusCode::OK, "{child}");
    let info = state
        .acp
        .session_info(child["thread_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(info.parent, Some(aow_protocol::InstanceRef::acp(thread)));
    assert_ne!(child["thread_id"], parent["thread_id"]);
    // Stop fixture adapters through the internal service after verifying HTTP ownership protection.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state
        .acp
        .delete_workspace(&repo.to_string_lossy())
        .await
        .unwrap();
}

#[test]
fn optional_slug_has_no_dangling_separator_and_normalizes_future_values() {
    for slug in [None, Some(""), Some("  ")] {
        let slug = workspace::normalize_slug(slug);
        assert_eq!(
            workspace::task_branch("task/{{task_id}}-{{slug}}", "abc", slug.as_deref()).unwrap(),
            "task/abc"
        );
    }
    let slug = workspace::normalize_slug(Some("  Review / UI Changes! "));
    assert_eq!(
        workspace::task_branch("task/{{task_id}}-{{slug}}", "abc", slug.as_deref()).unwrap(),
        "task/abc-review-ui-changes"
    );
    assert!(workspace::task_branch("static-branch", "abc", None).is_err());
}

pub(super) async fn workspace_fixture() -> (tempfile::TempDir, AppState, Project, Router) {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "Initial",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let mut state = AppState::new(Default::default());
    state.acp =
        aow_acp::AcpService::new(Arc::new(Host), Some(directory.path().join("acp"))).unwrap();
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: repo.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let app = routes().with_state(state.clone());
    (directory, state, project, app)
}

#[tokio::test]
async fn new_worktree_defaults_create_and_execute_without_client_configuration() {
    let (directory, state, project, app) = workspace_fixture().await;
    // Old project documents acquire the defaults without a configuration migration or editor.
    let mut legacy = serde_json::to_value(&state.aow.lock().unwrap().projects[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("task_worktree");
    let legacy: StoredProject = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        legacy.task_worktree.branch_template,
        "task/{{task_id}}-{{slug}}"
    );
    assert_eq!(
        legacy.task_worktree.worktree_parent_dir,
        "../.aow-worktrees"
    );
    let prepared = workspace::create_task_workspace(
        &state,
        &project.id,
        "auto-workspace",
        Some("inspect-repository"),
    )
    .await
    .unwrap();
    let request = serde_json::json!({"request_id":"auto-workspace", "project_id":project.id, "agent_runtime_id":"test", "workspace_mode":"new_worktree", "prompt":"Inspect repository"});
    let (status, task) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let target = directory
        .path()
        .join(".aow-worktrees")
        .join(&project.id)
        .join("auto-workspace");
    assert_eq!(task["workspace"], target.to_string_lossy().as_ref());
    assert_eq!(task["slug"], "inspect-repository");
    assert_eq!(task["workspace"], prepared);
    let branch = git_output(&target, &["branch", "--show-current"])
        .await
        .unwrap();
    assert_eq!(branch.trim(), "task/auto-workspace-inspect-repository");
    assert_eq!(
        git_output(&target, &["rev-parse", "HEAD"]).await.unwrap(),
        git_output(Path::new(&project.registered_path), &["rev-parse", "main"])
            .await
            .unwrap()
    );
    let (_, retry) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(retry, task);
    assert_eq!(state.aow.inner.tasks.list().unwrap().len(), 1);
    let mut changed = request;
    changed["slug"] = serde_json::json!("different");
    assert_eq!(
        call(&app, "POST", "/api/aow/tasks", changed).await.0,
        StatusCode::BAD_REQUEST
    );
    let info = state
        .acp
        .session_info(task["thread_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(info.cwd, target.to_string_lossy());
    let submitted = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(log) = tokio::fs::read_to_string(target.join("lineage-prompts.jsonl")).await {
                break log;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Task prompt must be submitted automatically");
    let prompts: Vec<serde_json::Value> = submitted
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(prompts.len(), 1, "Retry must not execute the task twice");
    assert_eq!(prompts[0]["prompt"][0]["text"], "Inspect repository");
    state
        .acp
        .delete_workspace(&target.to_string_lossy())
        .await
        .unwrap();
}

#[tokio::test]
async fn default_prefers_remote_head_and_future_slug_can_be_supplied() {
    let (_directory, state, project, app) = workspace_fixture().await;
    let cwd = Path::new(&project.registered_path);
    let remote_commit = git_output(cwd, &["rev-parse", "HEAD"]).await.unwrap();
    git_output(
        cwd,
        &[
            "update-ref",
            "refs/remotes/origin/trunk",
            remote_commit.trim(),
        ],
    )
    .await
    .unwrap();
    git_output(
        cwd,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    )
    .await
    .unwrap();
    git_output(
        cwd,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "Local advance",
        ],
    )
    .await
    .unwrap();
    let (status, task) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"with-slug", "project_id":project.id, "agent_runtime_id":"test", "workspace_mode":"new_worktree", "slug":"Review UI", "prompt":"Review"})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let target = Path::new(task["workspace"].as_str().unwrap());
    assert_eq!(
        git_output(target, &["rev-parse", "HEAD"]).await.unwrap(),
        remote_commit
    );
    assert_eq!(
        git_output(target, &["branch", "--show-current"])
            .await
            .unwrap()
            .trim(),
        "task/with-slug-review-ui"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state
        .acp
        .delete_workspace(&target.to_string_lossy())
        .await
        .unwrap();
}

#[tokio::test]
async fn checkout_failure_does_not_create_task_or_start_acp() {
    let (_directory, state, project, app) = workspace_fixture().await;
    state.aow.lock().unwrap().projects[0].task_worktree.base_ref = Some("missing-ref".into());
    let (status, _) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"fail-checkout", "project_id":project.id, "agent_runtime_id":"test", "workspace_mode":"new_worktree", "slug":"", "prompt":"Review"})).await;
    assert!(!status.is_success());
    assert!(state.aow.inner.tasks.list().unwrap().is_empty());
    assert!(state.acp.sessions(None).is_empty());
}

#[tokio::test]
async fn project_policy_can_resolve_custom_defaults_without_a_ui_editor() {
    let (directory, state, project, app) = workspace_fixture().await;
    {
        let mut registry = state.aow.lock().unwrap();
        let policy = &mut registry.projects[0].task_worktree;
        policy.worktree_parent_dir = "../managed-checkouts".into();
        policy.branch_template = "feature/{{task_id}}-{{slug}}".into();
        policy.base_ref = Some("main".into());
    }
    let (status, task) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"configured", "project_id":project.id, "agent_runtime_id":"test", "workspace_mode":"new_worktree", "slug":"", "prompt":"Review"})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let target = directory
        .path()
        .join("managed-checkouts")
        .join(&project.id)
        .join("configured");
    assert_eq!(task["workspace"], target.to_string_lossy().as_ref());
    assert_eq!(
        git_output(&target, &["branch", "--show-current"])
            .await
            .unwrap()
            .trim(),
        "feature/configured"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state
        .acp
        .delete_workspace(&target.to_string_lossy())
        .await
        .unwrap();
}

#[test]
fn automatic_slug_follows_paperclip_sanitization_and_keeps_optional_override() {
    assert_eq!(
        workspace::task_slug("\n # Fix Login / API!\nDetails below", None).as_deref(),
        Some("fix-login-api")
    );
    assert_eq!(
        workspace::task_slug(" -- Fix__LOGIN --- flow__ ", None).as_deref(),
        Some("fix__login-flow")
    );
    assert_eq!(workspace::task_slug("修复登录问题", None), None);
    assert_eq!(
        workspace::task_slug("Fix 登录 API", None).as_deref(),
        Some("fix-api")
    );
    assert_eq!(
        workspace::task_slug("Fix Login", Some("Custom_Slug")).as_deref(),
        Some("custom_slug")
    );
    assert_eq!(workspace::task_slug("Fix Login", Some("")), None);
    assert_eq!(workspace::task_slug("Fix Login", Some(" _-- ")), None);
    assert!(workspace::task_slug(&"a".repeat(200), None).unwrap().len() <= 64);
}
