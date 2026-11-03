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
                status: "todo".into(),
                prompt: id.into(),
                agent_id: "agent".into(),
                agent_role_id: None,
                is_router: false,
                source_task_id: None,
                source_thread_id: None,
                workspace_intent: None,
                target_task_id: None,
                target_revision: None,
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
        .map(|prompt| {
            prompt["prompt"].as_array().unwrap().last().unwrap()["text"]
                .as_str()
                .unwrap()
        })
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

async fn task_idle(state: &AppState, thread: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let session = state.acp.snapshot(thread).unwrap();
            let pending_run = state
                .aow
                .task_db()
                .active_runs()
                .unwrap()
                .iter()
                .any(|run| run.thread_id == thread);
            if session.status == "idle"
                && !pending_run
                && (session
                    .entries
                    .iter()
                    .any(|entry| entry.kind == "assistant")
                    || session.entries.is_empty())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn router_and_execution_tasks_share_delegation_with_isolation_and_provenance() {
    use serde_json::json;
    let (directory, mut state, original, _) = workspace_fixture().await;
    state.aow = AowManager::persistent(&directory.path().join("state")).unwrap();
    state.acp = aow_acp::AcpService::new(
        Arc::new(Host("codex.test")),
        Some(directory.path().join("routing-acp")),
    )
    .unwrap();
    let project = state
        .aow
        .register_project(RegisterProjectRequest {
            path: original.registered_path,
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    std::fs::write(
        Path::new(&project.registered_path).join(".git/info/exclude"),
        "lineage-prompts.jsonl\n",
    )
    .unwrap();
    let settings = state.aow.acp_settings().unwrap();
    state.aow.save_acp_settings(json!({"agent_servers":{"codex.test":{"type":"custom", "command":"node", "args":[
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")
    ]}}}).to_string(), settings.revision).await.unwrap();
    let app = routes()
        .merge(cli_routes())
        .merge(roles::routes())
        .merge(crate::acp::routes())
        .with_state(state.clone());
    let (status, role) = call(
        &app,
        "POST",
        "/api/aow/agent-roles",
        json!({
            "name":"Engineer", "description":"Implement and investigate",
            "runtime":{"mode":"acp","agent_id":"codex.test"},
            "home_layout":"codex", "identity":{"kind":"local_home","source":null},
            "instruction":"Return concrete findings", "skills":[]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{role}");
    let role_id = role["id"].as_str().unwrap();
    let route = format!("/api/aow/projects/{}/router", project.id);
    let (first, second) = tokio::join!(
        call(&app, "POST", &route, json!({"agent_id":"codex.test"})),
        call(&app, "POST", &route, json!({"agent_id":"codex.test"}))
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first.1, second.1);
    let router = first.1;
    assert_eq!(router["is_router"], true);
    let source = router["initial_thread_id"].as_str().unwrap();
    task_idle(&state, source).await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/acp/sessions/{source}"),
            json!({})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(
        state.acp.snapshot(source).unwrap().entries.is_empty(),
        "opening Router must not dispatch work"
    );
    let content = state.aow.acp_role_content(&state.acp, source, serde_json::from_value(json!([
        {"type":"text","text":format!("[@Engineer](mention://role/{role_id}) implement login")}
    ])).unwrap()).unwrap();
    assert!(
        content[0].data["text"]
            .as_str()
            .unwrap()
            .contains("persistent Router")
    );
    assert!(content[0].data["text"].as_str().unwrap().contains(role_id));
    assert!(
        !content[0].data["text"]
            .as_str()
            .unwrap()
            .contains("Return concrete findings"),
        "mentioned roles are queried on demand"
    );
    let request = json!({"request_id":"implementation","source_thread_id":source,"prompt":"Implement login","agent_role_id":role_id,"intent":"change"});
    let (status, implementation) = call(&app, "POST", "/v1/tasks", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{implementation}");
    assert_eq!(implementation["source_task_id"], router["id"]);
    assert_eq!(implementation["source_thread_id"], source);
    assert_eq!(implementation["workspace_mode"], "new_worktree");
    assert_ne!(implementation["workspace"], router["workspace"]);
    let (_, retry) = call(&app, "POST", "/v1/tasks", request.clone()).await;
    assert_eq!(retry, implementation);
    let mut changed = request;
    changed["prompt"] = json!("Different work");
    assert_eq!(
        call(&app, "POST", "/v1/tasks", changed).await.0,
        StatusCode::BAD_REQUEST
    );
    let worker = implementation["initial_thread_id"].as_str().unwrap();
    task_idle(&state, worker).await;
    let worker_session = state.acp.snapshot(worker).unwrap();
    assert!(
        worker_session.parent.is_none(),
        "delegation is distinct from runtime ownership"
    );
    assert!(
        worker_session.execution_environment.is_some(),
        "use the selected role's identity"
    );
    assert_eq!(
        worker_session.title, "Implement login",
        "instructions must not become the title"
    );
    let (status, helper) = call(&app, "POST", "/v1/tasks", json!({"request_id":"helper","source_thread_id":worker,"prompt":"Investigate the API","intent":"analyze"})).await;
    assert_eq!(status, StatusCode::OK, "{helper}");
    assert_eq!(helper["source_task_id"], "implementation");
    assert_eq!(helper["workspace"], router["workspace"]);
    assert!(
        helper["agent_role_id"].is_null(),
        "fallback executor must not inherit the sender's role"
    );
    let (status, review) = call(&app, "POST", "/v1/tasks", json!({"request_id":"review","source_thread_id":worker,"prompt":"Review login","agent_role_id":role_id,"intent":"review","target_task_id":"implementation"})).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["workspace"], implementation["workspace"]);
    assert_eq!(review["target_revision"].as_str().unwrap().len(), 40);
    task_idle(&state, review["initial_thread_id"].as_str().unwrap()).await;
    let (status, result) = call(
        &app,
        "GET",
        &format!("/v1/tasks/review?source_thread_id={worker}"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["runtime"]["response"], "Done");
    let (status, sent) = call(
        &app,
        "POST",
        "/v1/tasks/review/send",
        json!({"request_id":"error-handling","source_thread_id":worker,"prompt":"Check error handling too"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let review_thread = review["initial_thread_id"].as_str().unwrap();
    task_idle(&state, review_thread).await;
    state
        .acp
        .start_prompt(
            review_thread,
            serde_json::from_value(json!([{"type":"text","text":"wait"}])).unwrap(),
        )
        .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        "/v1/tasks/review/send",
        json!({"request_id":"queued","source_thread_id":worker,"prompt":"Do not duplicate this"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "busy targets accept a durable queued execution"
    );
    state.acp.cancel(review_thread).unwrap();
    task_idle(&state, review_thread).await;
    let mut foreign = state
        .aow
        .inner
        .tasks
        .list()
        .unwrap()
        .into_iter()
        .find(|task| task.id == "implementation")
        .unwrap();
    foreign.id = "foreign".into();
    foreign.project_id = "other-project".into();
    foreign.resources.clear();
    foreign.initial_thread_id = None;
    state.aow.inner.tasks.insert(&foreign).unwrap();
    for (method, path, body) in [
        (
            "GET",
            format!("/v1/tasks/foreign?source_thread_id={worker}"),
            json!({}),
        ),
        (
            "POST",
            "/v1/tasks/foreign/send".into(),
            json!({"request_id":"foreign-send","source_thread_id":worker,"prompt":"Cross-project message"}),
        ),
        (
            "POST",
            "/v1/tasks".into(),
            json!({"request_id":"foreign-review","source_thread_id":worker,"prompt":"Review","intent":"review","target_task_id":"foreign"}),
        ),
    ] {
        assert_eq!(
            call(&app, method, &path, body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    state.aow.inner.tasks.remove("foreign").unwrap();
    // Another role performs review inside the SAME business task, with a
    // different conversation and its own immutable execution context.
    let (status, reviewer) = call(&app, "POST", "/api/aow/agent-roles", json!({
        "name":"Reviewer", "description":"Review", "runtime":{"mode":"acp","agent_id":"codex.test"},
        "home_layout":"codex", "identity":{"kind":"local_home","source":null},
        "instruction":"Review the recorded revision", "skills":[]
    })).await;
    assert_eq!(status, StatusCode::OK, "{reviewer}");
    let request = json!({"request_id":"same-goal-review","source_thread_id":source,"prompt":"Review login","agent_role_id":reviewer["id"],"intent":"review"});
    let (status, accepted) = call(
        &app,
        "POST",
        "/v1/tasks/implementation/runs",
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let review_thread = accepted["thread_id"].as_str().unwrap();
    assert_ne!(review_thread, worker);
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/tasks/implementation/runs",
            request.clone()
        )
        .await
        .1["run_id"],
        accepted["run_id"]
    );
    let mut conflict = request.clone();
    conflict["intent"] = json!("change");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/implementation/runs", conflict)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    task_idle(&state, review_thread).await;
    let run = state
        .aow
        .task_db()
        .run(accepted["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let execution = run.execution.as_ref().unwrap();
    assert_eq!(execution.agent_role_id.as_deref(), reviewer["id"].as_str());
    assert_eq!(
        execution.workspace,
        implementation["workspace"].as_str().unwrap()
    );
    assert_eq!(execution.target_task_id.as_deref(), Some("implementation"));
    assert!(execution.target_revision.is_some());
    let instructions = state
        .aow
        .acp_role_content(
            &state.acp,
            review_thread,
            serde_json::from_value(json!([{"type":"text","text":"Continue review"}])).unwrap(),
        )
        .unwrap();
    assert!(instructions.iter().any(|block| {
        block.data["text"]
            .as_str()
            .is_some_and(|text| text.contains("Review the recorded revision"))
    }));
    assert!(instructions.iter().any(|block| {
        block.data["text"]
            .as_str()
            .is_some_and(|text| text.contains(execution.target_revision.as_deref().unwrap()))
    }));
    let mut next = request;
    next["request_id"] = json!("same-goal-review-2");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/implementation/runs", next)
            .await
            .1["thread_id"],
        review_thread,
        "reuse matching role/workspace/revision context"
    );
    task_idle(&state, review_thread).await;
    let fix = json!({"request_id":"same-goal-fix","source_thread_id":source,"prompt":"Fix review findings","agent_role_id":role_id,"intent":"change"});
    let (status, fixed) = call(&app, "POST", "/v1/tasks/implementation/runs", fix).await;
    assert_eq!(status, StatusCode::OK, "{fixed}");
    assert_eq!(
        fixed["thread_id"], worker,
        "developer retains its conversation and worktree"
    );
    task_idle(&state, worker).await;
    assert_eq!(
        state.aow.task_db().run(&run.id).unwrap().unwrap(),
        run,
        "later fixes cannot overwrite review results"
    );
    let (_, business) = call(&app, "GET", "/v1/tasks/implementation", json!({})).await;
    assert_eq!(business["task"]["status"], "in_progress");
    assert_eq!(
        business["task"]["agent_role_id"], role_id,
        "execution roles do not change task defaults"
    );
    for status in ["in_review", "done"] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/v1/tasks/implementation/status",
                json!({"status":status,"source_thread_id":source})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/tasks/implementation/send",
            json!({"request_id":"closed","prompt":"More work","source_thread_id":source})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/tasks/implementation/status",
            json!({"status":"todo","source_thread_id":source})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        state.aow.task_db().list().unwrap().len(),
        4,
        "review and fix add runs, not tasks"
    );
    // An analysis task can proceed to implementation without becoming a new task.
    // Its run gets an isolated workspace and review resolves that workspace.
    let (status, change) = call(&app, "POST", "/v1/tasks/helper/runs", json!({"request_id":"implement-after-analysis","source_thread_id":source,"prompt":"Implement the API","agent_role_id":role_id,"intent":"change"})).await;
    assert_eq!(status, StatusCode::OK, "{change}");
    let change_run = state
        .aow
        .task_db()
        .run(change["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let changed_workspace = &change_run.execution.as_ref().unwrap().workspace;
    assert_ne!(changed_workspace, helper["workspace"].as_str().unwrap());
    task_idle(&state, &change_run.thread_id).await;
    let mut scope_headers = axum::http::HeaderMap::new();
    scope_headers.insert("x-aow-task", "helper".parse().unwrap());
    assert_eq!(
        state
            .aow
            .task_scope(&scope_headers, Path::new(changed_workspace))
            .unwrap()
            .as_deref(),
        Some("helper")
    );
    let (status, reviewed) = call(&app, "POST", "/v1/tasks/helper/runs", json!({"request_id":"review-implemented-api","source_thread_id":source,"prompt":"Review the API","agent_role_id":reviewer["id"],"intent":"review"})).await;
    assert_eq!(status, StatusCode::OK, "{reviewed}");
    let review_run = state
        .aow
        .task_db()
        .run(reviewed["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(&review_run.execution.unwrap().workspace, changed_workspace);
    task_idle(&state, &review_run.thread_id).await;
    let (status, changed_review) = call(&app, "POST", "/v1/tasks/review/runs", json!({"request_id":"change-review-task","source_thread_id":source,"prompt":"Implement follow-up","intent":"change"})).await;
    assert_eq!(status, StatusCode::OK, "{changed_review}");
    let changed_review = state
        .aow
        .task_db()
        .run(changed_review["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_ne!(
        changed_review.execution.unwrap().workspace,
        implementation["workspace"].as_str().unwrap(),
        "a borrowed review workspace must not become this task's change workspace"
    );
    task_idle(&state, &changed_review.thread_id).await;
    std::fs::write(
        Path::new(implementation["workspace"].as_str().unwrap()).join("uncommitted.txt"),
        "changes",
    )
    .unwrap();
    let (status, _) = call(&app, "POST", "/v1/tasks", json!({"request_id":"dirty-review","source_thread_id":worker,"prompt":"Review dirty files","intent":"review","target_task_id":"implementation"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(state.aow.inner.tasks.list().unwrap().len(), 4);
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/aow/tasks/{}", router["id"].as_str().unwrap()),
            json!({"archived":true})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(
        state
            .aow
            .acp_role_content(
                &state.acp,
                worker,
                serde_json::from_value(
                    json!([{"type":"text","text":"[@missing](mention://role/missing) fix"}])
                )
                .unwrap()
            )
            .is_err()
    );
    for workspace in state.aow.project(&project.id).await.unwrap().worktrees {
        state.acp.delete_workspace(&workspace.path).await.unwrap();
    }
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
    assert_eq!(
        prompts[0]["prompt"].as_array().unwrap().last().unwrap()["text"],
        "Inspect repository"
    );
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
        serde_json::json!({"prompt":"[@Unknown](mention://role/missing) implement a feature"}),
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
