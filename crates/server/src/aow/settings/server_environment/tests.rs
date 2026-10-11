use std::os::unix::fs::{PermissionsExt, symlink};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

async fn call(app: &Router, content: Option<Value>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(if content.is_some() { "PUT" } else { "GET" })
                .uri("/api/aow/settings/server-environment")
                .header("content-type", "application/json")
                .body(Body::from(
                    content.map(|value| value.to_string()).unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn edits_the_actual_file_verbatim_and_can_create_or_clear_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(".config/aow/server.env");
    let app = router(path.clone());
    let (status, initial) = call(&app, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["path"], json!(path));
    assert_eq!(initial["exists"], false);
    assert_eq!(initial["content"], "");
    assert!(!path.exists(), "reading must not create a file");
    let content = "# 保留注释\r\n\r\nAOW_SERVER_PORT = '8283'\r\nKEY=\"$HOME=a=b\"\r\nKEY=last\r\n";
    let (status, saved) = call(
        &app,
        Some(json!({"content": content, "revision": initial["revision"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["content"], content);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(call(&app, None).await.1, saved);
    let (_, cleared) = call(
        &app,
        Some(json!({"content": "", "revision": saved["revision"]})),
    )
    .await;
    assert_eq!(cleared["exists"], true);
    assert_eq!(std::fs::read(&path).unwrap(), b"");
    assert_ne!(cleared["revision"], initial["revision"]);
}

#[tokio::test]
async fn external_changes_and_competing_editors_cannot_overwrite_the_last_save() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.env");
    std::fs::write(&path, "ORIGINAL=value\n").unwrap();
    let app = router(path.clone());
    let (_, initial) = call(&app, None).await;
    std::fs::write(&path, "EXTERNAL=updated\n").unwrap();
    let (status, _) = call(
        &app,
        Some(json!({"content": "STALE=value", "revision": initial["revision"]})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "EXTERNAL=updated\n"
    );
    let (_, fresh) = call(&app, None).await;
    let (first, second) = tokio::join!(
        call(
            &app,
            Some(json!({"content": "FIRST=1", "revision": fresh["revision"]}))
        ),
        call(
            &app,
            Some(json!({"content": "SECOND=2", "revision": fresh["revision"]}))
        ),
    );
    assert!(matches!(
        (first.0, second.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
}

#[tokio::test]
async fn invalid_content_and_file_failures_leave_the_original_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.env");
    let original = "SECRET=private\n";
    std::fs::write(&path, original).unwrap();
    let app = router(path.clone());
    let (_, initial) = call(&app, None).await;
    for content in ["SECRET=private\0".to_owned(), "x".repeat(MAX_BYTES + 1)] {
        let (status, error) = call(
            &app,
            Some(json!({"content": content, "revision": initial["revision"]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!error.to_string().contains("private"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
    std::fs::remove_file(&path).unwrap();
    let target = directory.path().join("target.env");
    std::fs::write(&target, original).unwrap();
    symlink(&target, &path).unwrap();
    assert_eq!(call(&app, None).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        call(
            &app,
            Some(json!({"content": "", "revision": initial["revision"]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), original);
    assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(call(&app, None).await.0, StatusCode::BAD_REQUEST);
}
