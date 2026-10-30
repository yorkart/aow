use super::tests::{call, stored, workspace_fixture};
use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn session_context_creates_children_and_supports_entire_task_lifecycle() {
    let (_directory, state, project, _) = workspace_fixture().await;
    let app = cli_routes().with_state(state.clone());
    let (status, root) = call(&app, "POST", "/v1/tasks", json!({"project_id":project.id,"agent_runtime_id":"test","request_id":"root-cli","task":"Root task"})).await;
    assert_eq!(status, StatusCode::OK, "{root}");
    assert_eq!(root["phase"], "ready");
    let native = root["session_id"]
        .as_str()
        .expect("Native identity must be bound before execution");
    assert_eq!(root["agent_runtime_id"], "test");
    assert!(root.get("agent_id").is_none());
    assert!(root["agent_role_id"].is_null());
    // A future role assignment is inherited, independently of the runtime configuration.
    // Existing role fields are nullable until role management is introduced.
    let (status, child) = call(&app, "POST", "/v1/tasks", json!({"from_session_id":native,"task":"Child review","worktree":"new","request_id":"child-cli"})).await;
    assert_eq!(status, StatusCode::OK, "{child}");
    assert_eq!(child["phase"], "ready");
    assert_eq!(child["parent_id"], root["id"]);
    assert_eq!(child["project_id"], project.id);
    assert_eq!(child["agent_runtime_id"], "test");
    assert_ne!(child["workspace"], root["workspace"]);
    let (status, listed) = call(
        &app,
        "GET",
        &format!("/v1/tasks?project_id={}&parent_id=root-cli", project.id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    let (_, found) = call(&app, "GET", "/v1/tasks/child-cli", json!({})).await;
    assert_eq!(found["session"]["thread_id"], child["thread_id"]);
    // Wait for the fixture's initial turn; submit never takes over a working agent.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state
            .acp
            .snapshot(child["thread_id"].as_str().unwrap())
            .unwrap()
            .status
            == "working"
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (status, submitted) = call(
        &app,
        "POST",
        "/v1/tasks/child-cli/submit",
        json!({"task":"Follow up"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted}");
    let (status, archived) = call(&app, "POST", "/v1/tasks/root-cli/archive", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert_eq!(archived["status"], "archived");
    let (_, listed) = call(
        &app,
        "GET",
        &format!("/v1/tasks?project_id={}", project.id),
        json!({}),
    )
    .await;
    assert_eq!(listed["items"], json!([]));
    let (_, listed) = call(
        &app,
        "GET",
        &format!("/v1/tasks?project_id={}&include_archived=true", project.id),
        json!({}),
    )
    .await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    assert!(std::path::Path::new(child["workspace"].as_str().unwrap()).exists());
    assert!(
        state
            .acp
            .session_info(child["thread_id"].as_str().unwrap())
            .is_err()
    );
    assert_eq!(
        call(&app, "POST", "/v1/tasks/root-cli/delete", json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/v1/tasks/child-cli", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn caller_lookup_rejects_missing_ambiguous_and_archived_native_ids() {
    let state = AppState::new(Default::default());
    let mut first = stored("first", None);
    first.session_id = Some("ambiguous".into());
    let mut second = stored("second", None);
    second.session_id = first.session_id.clone();
    second.agent_runtime_id = "other".into();
    for task in [&first, &second] {
        state.aow.inner.tasks.insert(task).unwrap();
    }
    let app = cli_routes().with_state(state.clone());
    for (session, expected) in [
        ("missing", StatusCode::NOT_FOUND),
        ("ambiguous", StatusCode::CONFLICT),
    ] {
        let (status, _) = call(
            &app,
            "POST",
            "/v1/tasks",
            json!({"from_session_id":session,"task":"Child"}),
        )
        .await;
        assert_eq!(status, expected);
    }
    state.aow.inner.tasks.finish("first", true).unwrap();
    state.aow.inner.tasks.finish("second", true).unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/tasks",
            json!({"from_session_id":"ambiguous","task":"Child"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn native_binding_is_refreshed_when_a_task_session_becomes_ready() {
    let (_directory, state, project, _) = workspace_fixture().await;
    let app = cli_routes().with_state(state.clone());
    let (_, ready) = call(&app, "POST", "/v1/tasks", json!({"project_id":project.id,"agent_runtime_id":"test","request_id":"bound","task":"Inspect"})).await;
    let row = state.aow.inner.tasks.list().unwrap().remove(0);
    assert_eq!(row.session_id.as_deref(), ready["session_id"].as_str());
    assert!(row.session_id.is_some());
    assert_eq!(row.thread_id, ready["thread_id"]);
    assert_eq!(row.agent_runtime_id, "test");
    // A stale persisted binding is reconciled to the actual native identity, never guessed from cwd.
    state
        .aow
        .inner
        .tasks
        .bind_session(&row.thread_id, &row.agent_runtime_id, "old-native")
        .unwrap();
    let (_, refreshed) = call(&app, "GET", "/v1/tasks/bound", json!({})).await;
    assert_eq!(refreshed["session_id"], ready["session_id"]);
    let (status, _) = call(
        &app,
        "POST",
        "/v1/tasks",
        json!({"from_session_id":"old-native","task":"Child"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    call(
        &app,
        "POST",
        "/v1/tasks/bound/delete",
        Value::Object(Default::default()),
    )
    .await;
}
