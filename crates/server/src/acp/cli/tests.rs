use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};
use tower::ServiceExt;

struct Host {
    command: String,
    delay_ms: u64,
}
#[async_trait::async_trait]
impl aow_acp::AcpHost for Host {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":self.command,"env":{"START_DELAY_MS":self.delay_ms.to_string()},"args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")]}}}).to_string())
    }
    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &FsPath, _: &FsPath) -> anyhow::Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &FsPath, _: &FsPath, _: &str) -> anyhow::Result<()> {
        unreachable!()
    }
}

async fn call(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
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
    let body =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    (status, body)
}

async fn fixture(command: &str) -> (tempfile::TempDir, AppState, Router, String, PathBuf) {
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
    state.acp = aow_acp::AcpService::new(
        Arc::new(Host {
            command: command.into(),
            delay_ms: 0,
        }),
        Some(directory.path().join("acp")),
    )
    .unwrap();
    state.instances = instances::InstanceStore::persistent(directory.path()).unwrap();
    let app = crate::build_router(state.clone());
    let (status, project) = call(&app, "POST", "/api/aow/projects", json!({"path":repo})).await;
    assert!(status.is_success(), "{project}");
    let project = project["id"].as_str().unwrap().to_owned();
    let app = routes()
        .route("/v1/instances/{reference}", get(instances::get))
        .with_state(state.clone());
    (directory, state, app, project, repo)
}

#[tokio::test]
async fn caller_context_errors_never_allocate_children() {
    let (_directory, state, app, project, repo) = fixture("node").await;
    let connection = state.acp.connect("test", repo.clone()).await.unwrap();
    let root = state.acp.new_session(&connection.id).await.unwrap();
    let request = json!({"agent_id":"test", "project_id":project, "cwd":repo,
        "timeout_seconds":10, "caller":{"session_id":root.session_id, "acp_connection_id":connection.id}});
    for caller in [
        Value::Null,
        json!({"session_id":""}),
        json!({"session_id":"unknown"}),
        json!({"session_id":"unknown", "acp_connection_id":connection.id}),
        json!({"session_id":root.session_id, "acp_connection_id":"missing"}),
    ] {
        let mut invalid = request.clone();
        invalid["caller"] = caller;
        let (status, body) = call(&app, "POST", "/v1/acp/sessions", invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(state.acp.sessions(None).len(), 1);
    }
    let mut conflict = request.clone();
    conflict["no_parent"] = json!(true);
    conflict["parent"] = json!({"kind":"acp", "id":root.thread_id});
    assert_eq!(
        call(&app, "POST", "/v1/acp/sessions", conflict).await.0,
        StatusCode::BAD_REQUEST
    );
    state.acp.close_session(&root.thread_id).await.unwrap();
    assert_eq!(
        call(&app, "POST", "/v1/acp/sessions", request).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(state.acp.sessions(None).len(), 1);
}

#[tokio::test]
async fn cli_creates_submits_traces_and_respects_browser_ownership() {
    let (_directory, state, app, project, repo) = fixture("node").await;
    let create = json!({"caller":{"session_id":"fixture"},"no_parent":true,"agent_id":"test","project_id":project,"cwd":repo,"timeout_seconds":10,"task":"hello"});
    let (status, first) = call(&app, "POST", "/v1/acp/sessions", create.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["phase"], "ready", "{first}");
    assert_eq!(first["task_submitted"], true);
    let id = first["thread_id"].as_str().unwrap();
    let root_info = state.acp.session_info(id).unwrap();
    assert_eq!(root_info.source, aow_acp::SessionSource::Cli);
    assert!(root_info.parent.is_none());
    assert!(!root_info.created_at.is_empty());
    let parent = aow_protocol::InstanceRef::acp(id);
    let mut create = create;
    create["no_parent"] = json!(false);
    create["caller"] = json!({"session_id": root_info.session_id,
        "acp_connection_id": state.acp.session_connection_info(id).unwrap().unwrap().id});
    create["task"] = Value::Null;
    let (_, child) = call(&app, "POST", "/v1/acp/sessions", create).await;
    assert_eq!(child["phase"], "ready", "{child}");
    assert_eq!(child["parent"], json!(parent));
    let child_id = child["thread_id"].as_str().unwrap();
    assert_eq!(
        state.acp.session_info(child_id).unwrap().source,
        aow_acp::SessionSource::Cli
    );
    let controller = state.acp_controls.session(child_id);
    let owner = controller.claim_cli().unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/acp/sessions/{child_id}/submit"),
        json!({"task":"next"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    drop(owner);
    state.acp.close_session(child_id).await.unwrap();
    let (status, submitted) = call(
        &app,
        "POST",
        &format!("/v1/acp/sessions/{child_id}/submit"),
        json!({"task":"next"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted}");
    state.acp.delete_session(id).await.unwrap();
    let (_, lineage) = call(
        &app,
        "GET",
        &format!("/v1/instances/acp:{child_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(lineage["ancestors"][0]["id"], id);
    assert_eq!(lineage["ancestors"][0]["deleted"], true);
    assert_eq!(
        state.acp.session_info(child_id).unwrap().parent,
        Some(parent)
    );
}

#[tokio::test]
async fn failed_startup_returns_identity_and_invalid_parent_allocates_nothing() {
    let (_directory, state, app, project, repo) = fixture("/missing/adapter").await;
    let mut request = json!({"caller":{"session_id":"fixture"},"agent_id":"test","project_id":project,"cwd":repo,"timeout_seconds":1,"parent":{"kind":"acp","id":"missing"}});
    let (status, _) = call(&app, "POST", "/v1/acp/sessions", request.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(state.acp.sessions(None).is_empty());
    request["parent"] = Value::Null;
    request["no_parent"] = json!(true);
    let (status, failed) = call(&app, "POST", "/v1/acp/sessions", request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(failed["phase"], "failed");
    let id = failed["thread_id"].as_str().unwrap();
    assert!(state.acp.session_info(id).is_ok());
    assert!(
        state
            .instances
            .snapshot()
            .unwrap()
            .contains_key(&aow_protocol::InstanceRef::acp(id))
    );
}

#[tokio::test]
async fn timed_out_creation_finishes_once_and_busy_submission_is_rejected() {
    let (_directory, mut state, _, project, repo) = fixture("node").await;
    state.acp = aow_acp::AcpService::new(
        Arc::new(Host {
            command: "node".into(),
            delay_ms: 1500,
        }),
        None,
    )
    .unwrap();
    let app = routes().with_state(state.clone());
    let (status, result) = call(&app, "POST", "/v1/acp/sessions", json!({"caller":{"session_id":"fixture"},"no_parent":true,"agent_id":"test","project_id":project,"cwd":repo,"timeout_seconds":1,"task":"wait"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["phase"], "starting", "{result}");
    let id = result["thread_id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.acp.snapshot(id).unwrap().status != "working" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/acp/sessions/{id}/submit"),
        json!({"task":"duplicate"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    state.acp.cancel(id).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.acp.snapshot(id).unwrap().status != "idle" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let prompts = std::fs::read_to_string(repo.join("lineage-prompts.jsonl")).unwrap();
    assert_eq!(prompts.lines().count(), 1);
}
