use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tower::ServiceExt;

struct Host;
#[async_trait::async_trait]
impl aow_acp::AcpHost for Host {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(
            json!({"agent_servers":{"test":{"type":"custom","command":"/missing/acp-adapter"}}})
                .to_string(),
        )
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
async fn image_prompt_body_limit_is_bounded_and_scoped_to_actions() {
    let app = crate::acp::routes().with_state(crate::AppState::new(Default::default()));
    for (path, bytes, expected) in [
        (
            "/api/acp/sessions/test/actions",
            3 * 1024 * 1024,
            StatusCode::CONFLICT,
        ),
        (
            "/api/acp/sessions/test/actions",
            33 * 1024 * 1024,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            "/api/acp/connections",
            3 * 1024 * 1024,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let body = json!({"action":"prompt", "content":[{"type":"image", "mimeType":"image/png", "data":"A".repeat(bytes)}]}).to_string();
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        // A supported-size prompt reaches ownership enforcement, never bypasses it.
        assert_eq!(response.status(), expected, "{path}: {bytes}");
    }
}

#[tokio::test]
async fn api_prepares_named_identity_and_reads_metadata_before_owned_startup() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().canonicalize().unwrap();
    let mut state = crate::AppState::new(Default::default());
    state.acp = aow_acp::AcpService::new(Arc::new(Host), Some(workspace.join("state"))).unwrap();
    let app = crate::acp::routes().with_state(state.clone());
    let prepared = app
        .clone()
        .oneshot(
            Request::post("/api/acp/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"agent_id":"test","cwd":workspace}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prepared.status(), StatusCode::OK);
    let prepared: Value =
        serde_json::from_slice(&to_bytes(prepared.into_body(), 65536).await.unwrap()).unwrap();
    let thread_id = prepared["thread_id"].as_str().unwrap();
    assert!(prepared["session_id"].is_null());
    assert!(prepared.get("id").is_none() && prepared.get("remote_id").is_none());
    let controller = state.acp_controls.session(thread_id);
    let token = controller.claim(uuid::Uuid::new_v4(), false).token.unwrap();
    for credential in [None, Some(token.as_str())] {
        let mut request = Request::post(format!("/api/acp/sessions/{thread_id}/actions"))
            .header("content-type", "application/json");
        if let Some(token) = credential {
            request = request.header("X-Aow-Acp-Control", token);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(r#"{"action":"resume"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if credential.is_none() {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            }
        );
    }
    for (suffix, expected) in [
        ("/metadata", prepared["cwd"].clone()),
        ("/connection", Value::Null),
        ("/logs", json!([])),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/api/acp/sessions/{thread_id}{suffix}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        if suffix == "/metadata" {
            assert_eq!(body["thread_id"], thread_id);
            assert_eq!(body["cwd"], expected);
        } else {
            assert_eq!(body, expected);
        }
    }
    assert_eq!(state.acp.sessions(None).len(), 1);
    assert!(state.acp.snapshot(thread_id).unwrap().error.is_none());
    assert!(
        state
            .acp
            .snapshot(thread_id)
            .unwrap()
            .recovery
            .unwrap()
            .error
            .is_some()
    );
    let current = state.acp.snapshot(thread_id).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/acp/sessions/{thread_id}?view=overview"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let overview: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(overview["recovery"]["phase"], "failed");
    assert!(overview["recovery"]["error"].is_string());
    assert!(overview["error"].is_null());
    assert_eq!(overview["overview"], json!([]));
    assert_eq!(overview["entries"], json!([]));
    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/acp/sessions/{thread_id}/process?turn_id=missing"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    for (generation, kind) in [
        (&*current.sync_generation, "unchanged"),
        ("previous-process", "snapshot"),
    ] {
        for view in ["", "&view=overview"] {
            let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/acp/sessions/{thread_id}/updates?generation={generation}&revision={}{view}",
                    current.revision
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(body["kind"], kind);
            if kind == "unchanged" {
                assert_eq!(body, json!({"kind":"unchanged"}));
            } else {
                assert_eq!(body["snapshot"]["thread_id"], thread_id);
                assert_eq!(body["snapshot"]["overview"].is_array(), !view.is_empty());
            }
        }
    }
}
