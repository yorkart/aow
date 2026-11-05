use crate::{AppState, build_router};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
#[tokio::test]
async fn remote_cli_requires_its_own_revocable_token_and_cannot_use_browser_apis() {
    let root = tempfile::tempdir().unwrap();
    crate::auth::write_credentials(root.path(), "cli-test", "test-password");
    let state = AppState::for_test_with_state_dir(
        Default::default(),
        root.path().into(),
        root.path().join("unused"),
    )
    .unwrap()
    .with_cli_callback("http://server:9800/aow", None)
    .unwrap();
    let grant = state.cli_access.issue(60).unwrap();
    let app = build_router(state.clone());
    for path in ["/api/cli/v1/projects", "/api/cli/v1/automations"] {
        let denied = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {}", grant.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(body, json!({"items":[]}));
    }
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/settings")
                .header("authorization", format!("Bearer {}", grant.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let browser = app
        .clone()
        .oneshot(
            Request::get("/api/cli/v1/projects")
                .header("authorization", format!("Bearer {}", grant.token))
                .header("origin", "https://attacker.invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(browser.status(), StatusCode::UNAUTHORIZED);
    state.cli_access.revoke(&grant.id).unwrap();
    let revoked = app
        .oneshot(
            Request::get("/api/cli/v1/projects")
                .header("authorization", format!("Bearer {}", grant.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
}
