use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::json;
use tower::ServiceExt;

fn headers(state: &ControlState) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTROL_HEADER,
        state.token.as_ref().unwrap().parse().unwrap(),
    );
    headers
}

#[tokio::test]
async fn one_owner_observers_and_explicit_takeover() {
    let controller = Controller::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let original = controller.claim(first, false);
    assert_eq!(controller.claim(second, false).state, "observing");
    assert_eq!(controller.claim(first, false).token, original.token);
    assert!(controller.authorize(&headers(&original), false).is_ok());
    let replacement = controller.claim(second, true);
    assert_ne!(replacement.token, original.token);
    assert_eq!(controller.state(first).state, "observing");
    assert!(controller.authorize(&headers(&original), false).is_err());
    assert!(controller.authorize(&headers(&replacement), false).is_ok());
    controller.release(first);
    assert_eq!(
        controller.state(second).token,
        replacement.token,
        "a late close cannot release the new owner"
    );
    controller.release(second);
    assert!(controller.authorize(&headers(&replacement), false).is_err());
    assert_eq!(
        controller.state(first).state,
        "observing",
        "vacancy does not automatically promote observers"
    );
}

#[tokio::test]
async fn queued_restores_recheck_ownership_without_blocking_permission_replies() {
    let controller = Arc::new(Controller::default());
    let original = controller.claim(Uuid::new_v4(), false);
    let original_headers = headers(&original);
    let admitted = controller
        .authorize_restore(&original_headers)
        .await
        .unwrap();
    assert!(
        controller.authorize(&original_headers, false).is_ok(),
        "permission replies remain available during restore"
    );
    let queued_controller = controller.clone();
    let queued = tokio::spawn(async move {
        queued_controller
            .authorize_restore(&original_headers)
            .await
            .is_err()
    });
    let replacement = controller.claim(Uuid::new_v4(), true);
    drop(admitted);
    assert!(queued.await.unwrap());
    assert!(controller.authorize(&headers(&replacement), false).is_ok());
}

#[tokio::test]
async fn credentials_are_scoped_to_session_and_recycled_views_cannot_reuse_them() {
    let controls = Controls::default();
    let first = controls.session("first");
    assert!(Arc::ptr_eq(&first, &controls.session("first")));
    let original = first.claim(Uuid::new_v4(), false);
    let second = controls.session("second");
    assert!(second.authorize(&headers(&original), false).is_err());
    assert!(second.authorize(&HeaderMap::new(), true).is_ok());
    drop(first);
    assert!(
        controls
            .session("first")
            .authorize(&headers(&original), false)
            .is_err()
    );
}

#[tokio::test]
async fn every_http_session_action_requires_current_control() {
    let state = crate::AppState::new(std::path::PathBuf::new());
    let controller = state.acp_controls.session("session");
    let stale = headers(&controller.claim(Uuid::new_v4(), false));
    controller.claim(Uuid::new_v4(), true);
    let router = crate::acp::routes().with_state(state);
    for action in [
        json!({"action":"prompt","content":[{"type":"text","text":"hello"}]}),
        json!({"action":"steer","content":[{"type":"text","text":"hello"}]}),
        json!({"action":"resume"}),
        json!({"action":"cancel"}),
        json!({"action":"close"}),
        json!({"action":"set_mode","mode_id":"mode"}),
        json!({"action":"set_config","config_id":"model","value":"model"}),
        json!({"action":"answer","request_id":"permission","response":{}}),
        json!({"action":"dismiss_notice","notice_id":"notice"}),
        json!({"action":"clear_plan"}),
    ] {
        for headers in [&HeaderMap::new(), &stale] {
            let mut request = Request::post("/api/acp/sessions/session/actions")
                .body(Body::from(action.to_string()))
                .unwrap();
            *request.headers_mut() = headers.clone();
            request
                .headers_mut()
                .insert("content-type", "application/json".parse().unwrap());
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT, "{action}");
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
                "acp_control_required"
            );
        }
    }
    let response = router
        .oneshot(
            Request::delete("/api/acp/sessions/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn deletion_waits_for_creation_and_rechecks_control_after_takeover() {
    let controller = Arc::new(Controller::default());
    let owner = controller.claim(Uuid::new_v4(), false);
    let owner_headers = headers(&owner);
    let creation = controller.authorize_restore(&owner_headers).await.unwrap();
    let deleting = controller.clone();
    let deletion_headers = owner_headers.clone();
    let deletion = tokio::spawn(async move {
        deleting
            .authorize_lifecycle(&deletion_headers, true)
            .await
            .is_ok()
    });
    tokio::task::yield_now().await;
    assert!(!deletion.is_finished());
    controller.claim(Uuid::new_v4(), true);
    drop(creation);
    assert!(!deletion.await.unwrap());
    assert!(
        Controller::default()
            .authorize_lifecycle(&HeaderMap::new(), true)
            .await
            .is_ok()
    );
}
