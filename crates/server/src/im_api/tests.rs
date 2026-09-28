use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

#[tokio::test]
async fn all_im_routes_require_account_authentication() {
    let root = tempfile::tempdir().unwrap();
    crate::auth::write_credentials(root.path(), "admin", "test-password");
    let mut state = AppState::new(root.path().join("frontend"));
    state.auth = crate::auth::AuthService::persistent(root.path());
    let app = crate::build_router(state);
    for (method, path) in [
        ("PUT", "/api/aow/im/feishu"),
        ("DELETE", "/api/aow/im/feishu"),
        ("GET", "/api/aow/im/wechat"),
        ("DELETE", "/api/aow/im/wechat"),
        ("POST", "/api/aow/im/wechat/login"),
        ("POST", "/api/aow/im/wechat/login/id"),
        ("DELETE", "/api/aow/im/wechat/login/id"),
        ("POST", "/api/aow/im/wechat/test"),
        ("PUT", "/api/aow/im/wechat/test/receipt"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn provider_api_keeps_secrets_private_and_errors_are_not_cached() {
    let app = crate::build_router(AppState::new("/unused".into()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/aow/im/feishu")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"app_id":"cli_test","app_secret":"private-secret"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 10000).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private-secret"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/aow/im/wechat/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn test_receipt_requires_a_bound_provider() {
    let response = crate::build_router(AppState::new("/unused".into()))
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/aow/im/wechat/test/receipt")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"test_id":"old-test","received":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
}
