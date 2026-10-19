use crate::{AppState, build_router};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn search_route_returns_nested_results_and_honors_early_cancellation() {
    let root = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("nested/file.txt"), "hello needle").unwrap();
    let app = build_router(AppState::new(root.path().into()));
    let id = uuid::Uuid::new_v4();
    let query = |id: uuid::Uuid| {
        Request::post("/api/fs/search")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "request_id": id.to_string(), "root": root.path(), "query": "needle", "mode": "content"
                })
                .to_string(),
            ))
            .unwrap()
    };
    let response = app.clone().oneshot(query(id)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(json["files"][0]["relative_path"], "nested/file.txt");
    let id = uuid::Uuid::new_v4();
    let cancelled = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/fs/search/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        app.oneshot(query(id)).await.unwrap().status(),
        StatusCode::CONFLICT
    );
}
