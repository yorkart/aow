use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

fn spec(id: &str) -> Spec {
    Spec {
        id: id.into(),
        kind: "example.run",
        source: "cli",
        title: "示例操作".into(),
        project_id: Some("project".into()),
        resource: None,
        total: Some(2),
    }
}

#[test]
fn live_handles_finish_once_and_dropped_workers_are_interrupted() {
    let service = OperationService::in_memory();
    let operation = service.begin(spec("first"));
    operation.progress("第一步", Some(1));
    let observer = operation.clone();
    drop(operation);
    assert_eq!(service.snapshot().operations[0].outcome, None);
    observer.finish(Outcome::Succeeded, "完成");
    observer.progress("late callback", Some(0));
    drop(observer);
    let snapshot = service.snapshot();
    assert_eq!(snapshot.operations[0].outcome, Some(Outcome::Succeeded));
    assert_eq!(snapshot.operations[0].completed, 1);
    assert_eq!(snapshot.operations[0].message, "完成");
    drop(service.begin(spec("second")));
    assert_eq!(
        service.snapshot().operations[1].outcome,
        Some(Outcome::Interrupted)
    );
}

#[test]
fn recently_finished_long_operations_survive_the_completed_cache_limit() {
    let service = OperationService::in_memory();
    let long = service.begin(spec("long"));
    for index in 0..RECENT_LIMIT + 10 {
        service
            .begin(spec(&format!("short-{index}")))
            .finish(Outcome::Succeeded, "完成");
    }
    assert_eq!(service.snapshot().operations.len(), RECENT_LIMIT + 1);
    long.finish(Outcome::Succeeded, "长任务完成");
    let snapshot = service.snapshot();
    assert_eq!(snapshot.operations.len(), RECENT_LIMIT);
    assert!(
        snapshot.operations.iter().any(
            |operation| operation.id == "long" && operation.outcome == Some(Outcome::Succeeded)
        )
    );
}

#[test]
fn history_survives_restart_without_restoring_live_operations() {
    let dir = tempfile::tempdir().unwrap();
    let service = OperationService::persistent(dir.path()).unwrap();
    let operation = service.begin(spec("test"));
    operation.event(Level::Info, "步骤已完成", Some(2));
    operation.finish(Outcome::Succeeded, "完成");
    drop(operation);
    drop(service); // drains the logging worker
    let page = Reader::new(dir.path())
        .read(&ReadOptions::default())
        .unwrap();
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.items[0].outcome, Some(Outcome::Succeeded));
    assert_eq!(page.items[2].event, "started");
    let restarted = OperationService::persistent(dir.path()).unwrap();
    assert!(restarted.snapshot().operations.is_empty());
    assert_ne!(restarted.snapshot().boot_id, page.items[0].boot_id);
}

#[tokio::test]
async fn history_api_filters_pages_validates_cursors_and_uses_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let service = OperationService::persistent(dir.path()).unwrap();
    let operation = service.begin(spec("op-one"));
    operation.finish(Outcome::Succeeded, "完成");
    drop(operation);
    drop(service);
    let mut state = AppState::new(Default::default());
    state.operations = OperationService::persistent(dir.path()).unwrap();
    let router = crate::build_router(state.clone());
    let response = router
        .clone()
        .oneshot(
            Request::get("/api/operation-logs?operation_id=op-one&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: Page =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event, "finished");
    let cursor = serde_json::to_string(&page.next_cursor.unwrap()).unwrap();
    let query: String = reqwest::Url::parse_with_params(
        "http://localhost/api/operation-logs",
        &[("cursor", cursor)],
    )
    .unwrap()
    .query()
    .unwrap()
    .into();
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/api/operation-logs?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: Page =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(page.items[0].event, "started");
    for path in [
        "/api/operation-logs?limit=0",
        "/api/operation-logs?cursor=garbage",
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    state.auth = crate::auth::PinAuth::persistent(dir.path());
    let protected = crate::build_router(state);
    for path in [
        "/api/operation-logs",
        "/api/operations/active",
        "/api/operations/stream",
    ] {
        assert!(
            !protected
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
}

#[tokio::test]
async fn reconnect_stream_starts_with_current_snapshot() {
    use futures_util::StreamExt;
    let state = AppState::new(Default::default());
    let operation = state.operations.begin(spec("live"));
    operation.progress("正在执行", Some(1));
    let response = crate::build_router(state.clone())
        .oneshot(
            Request::get("/api/operations/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(first.to_vec()).unwrap();
    assert!(text.contains("event: operations"));
    assert!(text.contains("正在执行"));
    operation.finish(Outcome::Succeeded, "完成");
    let second = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8(second.to_vec())
            .unwrap()
            .contains("succeeded")
    );
}
