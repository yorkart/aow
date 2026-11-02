use super::*;
use aow_db::tasks::TaskRecord;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

async fn call(
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

#[tokio::test]
async fn task_dependencies_do_not_cascade_archive_or_delete() {
    let state = AppState::new(Default::default());
    for (id, dependencies) in [("a", vec![]), ("b", vec!["a".into()])] {
        state
            .aow
            .inner
            .tasks
            .insert(&TaskRecord {
                id: id.into(),
                project_id: "project".into(),
                prompt: id.into(),
                agent_id: "agent".into(),
                agent_role_id: None,
                workspace: "/repo".into(),
                workspace_mode: "existing".into(),
                slug: None,
                initial_thread_id: None,
                pinned: false,
                archived: false,
                dependencies,
                resources: vec![],
            })
            .unwrap();
    }
    let app = routes().with_state(state.clone());
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/aow/tasks/a",
            serde_json::json!({"dependencies":["b"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/aow/tasks/a",
            serde_json::json!({"archived":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, tasks) = call(&app, "GET", "/api/aow/tasks", serde_json::json!({})).await;
    assert_eq!(tasks[0]["archived"], true);
    assert_eq!(tasks[1]["archived"], false);
    assert!(tasks[1].get("parent_id").is_none());
    assert_eq!(
        call(&app, "DELETE", "/api/aow/tasks/a", serde_json::json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, tasks) = call(&app, "GET", "/api/aow/tasks", serde_json::json!({})).await;
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["id"], "b");
}

#[tokio::test]
async fn invalid_task_creation_does_not_allocate_a_session() {
    let state = AppState::new(Default::default());
    let app = routes().with_state(state.clone());
    for body in [
        serde_json::json!({"request_id":"task", "project_id":"missing", "prompt":"", "agent_id":"agent", "workspace_mode":"existing", "workspace":"/repo"}),
        serde_json::json!({"request_id":"task", "project_id":"missing", "prompt":"hello", "agent_id":"agent", "workspace_mode":"existing", "workspace":"/repo", "dependencies":["missing"]}),
    ] {
        assert_eq!(
            call(&app, "POST", "/api/aow/tasks", body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert!(state.aow.inner.tasks.list().unwrap().is_empty());
    assert!(state.acp.sessions(None).is_empty());
}

struct Host(&'static str);
#[async_trait::async_trait]
impl aow_acp::AcpHost for Host {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(serde_json::json!({"agent_servers":{(self.0):{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")]}}}).to_string())
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
async fn creating_tasks_and_helper_sessions_keeps_dependencies_out_of_runtime_lineage() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git::git_output(&repo, &["init", "-b", "main"])
        .await
        .unwrap();
    let mut state = AppState::new(Default::default());
    state.acp =
        aow_acp::AcpService::new(Arc::new(Host("test")), Some(directory.path().join("acp")))
            .unwrap();
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
    let request = serde_json::json!({"request_id":"a", "project_id":project.id, "agent_id":"test", "workspace_mode":"existing", "workspace":repo, "prompt":"Inspect this repository"});
    let (status, first) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, repeated) = call(&app, "POST", "/api/aow/tasks", request).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(first, repeated);
    let (status, second) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"b", "project_id":project.id, "agent_id":"test", "workspace_mode":"existing", "workspace":repo, "prompt":"Review", "dependencies":["a"]})).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert!(
        state
            .acp
            .session_info(second["initial_thread_id"].as_str().unwrap())
            .unwrap()
            .parent
            .is_none()
    );
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/acp/sessions")
                    .header("content-type", "application/json")
                    .header("x-aow-task", "a")
                    .body(Body::from(
                        serde_json::json!({"agent_id":"test", "cwd":repo}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let tasks = state.aow.inner.tasks.list().unwrap();
    assert_eq!(tasks[0].resources.len(), 3);
    assert_eq!(tasks[1].resources.len(), 1);
    assert_eq!(tasks[0].workspace, tasks[1].workspace);
    let prompts = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let log =
                std::fs::read_to_string(repo.join("lineage-prompts.jsonl")).unwrap_or_default();
            let prompts: Vec<serde_json::Value> = log
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            if prompts.len() == 2 {
                break prompts;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("each task submits its initial prompt once, including after a retry");
    let mut texts: Vec<_> = prompts
        .iter()
        .map(|prompt| prompt["prompt"][0]["text"].as_str().unwrap())
        .collect();
    texts.sort();
    assert_eq!(texts, ["Inspect this repository", "Review"]);
    // Lifecycle remains ordinary ACP behavior: deleting a session leaves its task intact.
    let helper = &tasks[0].resources[1].id;
    let (status, payload) = call(
        &app,
        "DELETE",
        &format!("/api/acp/sessions/{helper}"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(state.aow.inner.tasks.list().unwrap().len(), 2);
    call(
        &app,
        "PATCH",
        "/api/aow/tasks/a",
        serde_json::json!({"archived":true}),
    )
    .await;
    assert!(
        state
            .acp
            .session_info(first["initial_thread_id"].as_str().unwrap())
            .is_ok()
    );
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-aow-task", "a".parse().unwrap());
    assert!(state.aow.task_scope(&headers, &repo).is_err());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state
        .acp
        .delete_workspace(&repo.canonicalize().unwrap().to_string_lossy())
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
    assert!(workspace::task_branch("{{task_id}}-{{unknown}}", "abc", None).is_err());
}

async fn workspace_fixture() -> (tempfile::TempDir, AppState, Project, Router) {
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
        aow_acp::AcpService::new(Arc::new(Host("test")), Some(directory.path().join("acp")))
            .unwrap();
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
    // Project documents acquire the defaults without a configuration migration or editor.
    let mut stored = serde_json::to_value(&state.aow.lock().unwrap().projects[0]).unwrap();
    stored.as_object_mut().unwrap().remove("task_worktree");
    let stored: StoredProject = serde_json::from_value(stored).unwrap();
    assert_eq!(
        stored.task_worktree.branch_template,
        "task/{{task_id}}-{{slug}}"
    );
    assert_eq!(
        stored.task_worktree.worktree_parent_dir,
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
    let request = serde_json::json!({"request_id":"auto-workspace", "project_id":project.id, "agent_id":"test", "workspace_mode":"new_worktree", "prompt":"Inspect repository"});
    let (status, task) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let target = directory
        .path()
        .canonicalize()
        .unwrap()
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
    let mut changed_mode = request.clone();
    changed_mode["workspace_mode"] = serde_json::json!("existing");
    changed_mode["workspace"] = task["workspace"].clone();
    assert_eq!(
        call(&app, "POST", "/api/aow/tasks", changed_mode).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        state
            .aow
            .project(&project.id)
            .await
            .unwrap()
            .worktrees
            .len(),
        2
    );
    let mut changed = request;
    changed["slug"] = serde_json::json!("different");
    assert_eq!(
        call(&app, "POST", "/api/aow/tasks", changed).await.0,
        StatusCode::BAD_REQUEST
    );
    let info = state
        .acp
        .session_info(task["initial_thread_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(info.cwd, target.to_string_lossy());
    let submitted = tokio::time::timeout(std::time::Duration::from_secs(10), async {
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
    let (status, task) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"with-slug", "project_id":project.id, "agent_id":"test", "workspace_mode":"new_worktree", "slug":"Review UI", "prompt":"Review"})).await;
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
    let (status, _) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"fail-checkout", "project_id":project.id, "agent_id":"test", "workspace_mode":"new_worktree", "slug":"", "prompt":"Review"})).await;
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
    let (status, task) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"configured", "project_id":project.id, "agent_id":"test", "workspace_mode":"new_worktree", "slug":"", "prompt":"Review"})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    let target = directory
        .path()
        .canonicalize()
        .unwrap()
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

#[tokio::test]
async fn invalid_creation_intent_does_not_create_a_checkout() {
    let (directory, state, project, app) = workspace_fixture().await;
    let request = serde_json::json!({"request_id":"invalid", "project_id":project.id, "agent_id":"test", "workspace_mode":"new_worktree", "prompt":"Review"});
    for changes in [
        serde_json::json!({"workspace_mode":"unknown"}),
        serde_json::json!({"workspace_mode":"existing"}),
        serde_json::json!({"workspace_mode":"existing", "workspace":" "}),
        serde_json::json!({"workspace":project.registered_path}),
        serde_json::json!({"request_id":"../escape"}),
        serde_json::json!({"agent_id":"missing"}),
        serde_json::json!({"dependencies":["missing"]}),
    ] {
        let mut input = request.clone();
        input
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        let (status, payload) = call(&app, "POST", "/api/aow/tasks", input).await;
        assert!(!status.is_success(), "{payload}");
        assert!(state.aow.inner.tasks.list().unwrap().is_empty());
        assert!(state.acp.sessions(None).is_empty());
        assert!(!directory.path().join(".aow-worktrees").exists());
    }
}

#[tokio::test]
async fn task_worktree_policy_rejects_paths_inside_checkouts_including_symlinks() {
    let (directory, state, project, _app) = workspace_fixture().await;
    let symlink = directory.path().join("alias");
    std::os::unix::fs::symlink(&project.registered_path, &symlink).unwrap();
    for parent in [
        ".".to_owned(),
        "../repo/nested".into(),
        symlink.to_string_lossy().into_owned(),
    ] {
        state.aow.lock().unwrap().projects[0]
            .task_worktree
            .worktree_parent_dir = parent;
        assert!(
            workspace::create_task_workspace(&state, &project.id, "unsafe", None)
                .await
                .is_err()
        );
        assert_eq!(
            state
                .aow
                .project(&project.id)
                .await
                .unwrap()
                .worktrees
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn task_role_reuses_acp_configuration_and_retries_without_creating_another_home_or_session() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git::git_output(&repo, &["init", "-b", "main"])
        .await
        .unwrap();
    let mut state = AppState::new(Default::default());
    state.aow = AowManager::persistent(&directory.path().join("aow")).unwrap();
    state.acp = aow_acp::AcpService::new(
        Arc::new(Host("codex.test")),
        Some(directory.path().join("acp")),
    )
    .unwrap();
    let settings = state.aow.acp_settings().unwrap();
    state
        .aow
        .save_acp_settings(
            aow_acp::AcpHost::settings(&Host("codex.test"))
                .await
                .unwrap(),
            settings.revision,
        )
        .await
        .unwrap();
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: repo.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let app = super::super::api::routes().with_state(state.clone());
    let (status, role) = call(
        &app,
        "POST",
        "/api/aow/agent-roles",
        serde_json::json!({
            "name":"Reviewer", "runtime":{"mode":"acp","agent_id":"codex.test"}, "home_layout":"codex",
            "identity":{"kind":"local_home","source":null}, "instruction":"Review only", "skills":[]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{role}");
    let request = serde_json::json!({"request_id":"role-task", "project_id":project.id, "prompt":"Check this change", "agent_role_id":role["id"], "workspace_mode":"existing", "workspace":repo});
    let (status, task) = call(&app, "POST", "/api/aow/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(task["agent_id"], "codex.test");
    assert_eq!(task["agent_role_id"], role["id"]);
    let id = task["initial_thread_id"].as_str().unwrap();
    let session = state.acp.snapshot(id).unwrap();
    assert!(session.parent.is_none());
    assert_eq!(
        session.execution_environment.as_ref().unwrap().home,
        state.aow.role_home(role["id"].as_str().unwrap()).unwrap()
    );
    assert_eq!(call(&app, "POST", "/api/aow/tasks", request).await.1, task);
    assert_eq!(state.acp.sessions(None).len(), 1);
    assert_eq!(state.acp.agents().await.unwrap().len(), 1);
    let home = state.aow.role_home(role["id"].as_str().unwrap()).unwrap();
    assert!(home.is_dir());
    let reopened =
        aow_db::tasks::TaskDb::open(Some(&directory.path().join("aow/state/tasks.sqlite3")))
            .unwrap();
    assert_eq!(
        reopened.list().unwrap()[0].agent_role_id.as_deref(),
        role["id"].as_str()
    );
}
