use super::*;
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
fn stored(id: &str, parent: Option<&str>) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        project_id: "project".into(),
        parent_id: parent.map(str::to_owned),
        prompt: "Review".into(),
        agent_id: "test".into(),
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
    let (status, _) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"project_id":"missing", "prompt":" ", "agent_id":"missing", "workspace":"/repo"})).await;
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
    let request = serde_json::json!({"request_id":"create-root", "project_id":project.id, "agent_id":"test", "workspace":repo, "prompt":"Inspect this repository"});
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
    let (status, child) = call(&app, "POST", "/api/aow/tasks", serde_json::json!({"request_id":"create-child", "parent_id":parent["id"], "project_id":project.id, "agent_id":"test", "workspace":repo, "prompt":"Review the result"})).await;
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
