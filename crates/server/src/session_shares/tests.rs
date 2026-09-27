use super::*;
use aow_agents::sessions::AgentSession;
use axum::{
    body::{Body, to_bytes},
    http::{
        Request,
        header::{COOKIE, SET_COOKIE},
    },
};
use serde_json::{Value, json};
use tower::ServiceExt;

const FIRST_TURN: &str = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"first question\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"last_agent_message\":\"first answer\"}}\n";

fn state(root: &Path) -> AppState {
    let mut state = AppState::new(root.join("frontend"));
    state.auth = crate::auth::PinAuth::persistent(root);
    state.session_shares = SessionShares::persistent(root).unwrap();
    state
}

fn locator(root: &Path) -> AgentSessionLocator {
    AgentSessionLocator {
        agent: "codex",
        session_id: "test-session".into(),
        title: "Shared conversation".into(),
        cwd: root.to_owned(),
        transcript_path: root.join("session.jsonl"),
        trusted_root: root.to_owned(),
    }
}

fn cache_locator(state: &AppState, locator: AgentSessionLocator) {
    let session = AgentSession::new(locator.clone(), chrono::Utc::now(), chrono::Utc::now());
    state
        .aow
        .cache_session_locator(&locator.cwd, &session)
        .unwrap();
}

async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    payload: Value,
) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    app.clone()
        .oneshot(
            request
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn share_links_follow_the_current_mount_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("session.jsonl"), FIRST_TURN).unwrap();
    std::fs::write(
        root.join(crate::auth::PIN_HASH_FILE),
        format!("{:x}", md5::compute(b"123456")),
    )
    .unwrap();
    let initial = state(&root).with_base_path(crate::BasePath::parse("/tools/aow").unwrap());
    cache_locator(&initial, locator(&root));
    let cookie = format!(
        "{}={}",
        initial.base_path.cookie_name(),
        initial.auth.login("123456").unwrap()
    );
    let app = crate::build_router(initial);
    let share = body(
        request(
            &app,
            "POST",
            "/tools/aow/api/aow/agent-sessions/test-session/share",
            Some(&cookie),
            json!({"agent":"codex", "worktree_path":root}),
        )
        .await,
    )
    .await;
    let token = share["path"]
        .as_str()
        .unwrap()
        .strip_prefix("/tools/aow/share/")
        .unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/tools/aow/api/public/session-shares/{token}"),
            None,
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );

    let restored = state(&root).with_base_path(crate::BasePath::parse("/other").unwrap());
    let cookie = format!(
        "{}={}",
        restored.base_path.cookie_name(),
        restored.auth.login("123456").unwrap()
    );
    let app = crate::build_router(restored);
    let info = body(
        request(
            &app,
            "GET",
            "/other/api/aow/agent-sessions/test-session/share?agent=codex",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(info["path"], format!("/other/share/{token}"));
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/other/api/public/session-shares/{token}"),
            None,
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn sharing_requires_login_but_live_reading_survives_restart_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("frontend")).unwrap();
    std::fs::write(root.join("frontend/index.html"), "<div id=root></div>").unwrap();
    std::fs::write(
        root.join(crate::auth::PIN_HASH_FILE),
        format!("{:x}", md5::compute(b"123456")),
    )
    .unwrap();
    std::fs::write(root.join("session.jsonl"), FIRST_TURN).unwrap();
    let initial = state(&root);
    let cookie = format!("aow_session={}", initial.auth.login("123456").unwrap());
    cache_locator(&initial, locator(&root));
    let app = crate::build_router(initial);
    let create_path = "/api/aow/agent-sessions/test-session/share";
    let payload = json!({"agent":"codex", "worktree_path":root});
    assert_eq!(
        request(&app, "POST", create_path, None, payload.clone())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let created = request(&app, "POST", create_path, Some(&cookie), payload.clone()).await;
    assert_eq!(created.status(), StatusCode::OK);
    let share = body(created).await;
    assert_eq!(
        body(request(&app, "POST", create_path, Some(&cookie), payload.clone()).await).await,
        share
    );
    assert_eq!(
        body(
            request(
                &app,
                "GET",
                &format!("{create_path}?agent=codex"),
                Some(&cookie),
                Value::Null
            )
            .await
        )
        .await,
        share
    );
    let token = share["path"]
        .as_str()
        .unwrap()
        .strip_prefix("/share/")
        .unwrap();
    let public_path = format!("/api/public/session-shares/{token}");
    let read = request(&app, "GET", &public_path, None, Value::Null).await;
    assert_eq!(read.status(), StatusCode::OK);
    assert!(!read.headers().contains_key(SET_COOKIE));
    assert_eq!(read.headers()[CACHE_CONTROL], "no-store");
    assert_eq!(
        body(read).await["turns"][0]["final"]["text"],
        "first answer"
    );
    let page = request(
        &app,
        "GET",
        share["path"].as_str().unwrap(),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()["referrer-policy"], "no-referrer");
    assert_eq!(page.headers()["x-robots-tag"], "noindex, nofollow");

    for path in [
        "/api/fs/tree",
        "/api/aow/agent-sessions",
        "/api/terminal/tabs",
        create_path,
    ] {
        let forged_cookie = format!("aow_session={token}");
        assert_eq!(
            request(&app, "GET", path, Some(&forged_cookie), Value::Null)
                .await
                .status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    assert_eq!(
        request(&app, "DELETE", &public_path, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{public_path}/extra"),
            None,
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/public/session-shares/unknown",
            None,
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let mut untrusted_payload = payload.clone();
    untrusted_payload["transcript_path"] = json!("/etc/passwd");
    assert_eq!(
        request(&app, "POST", create_path, Some(&cookie), untrusted_payload)
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/aow/agent-sessions/unscanned/share",
            Some(&cookie),
            payload.clone()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // Fresh login state and no in-memory locator cache: the public link still works.
    let restarted = state(&root);
    let new_cookie = format!("aow_session={}", restarted.auth.login("123456").unwrap());
    let app = crate::build_router(restarted.clone());
    assert_eq!(
        request(&app, "GET", &public_path, None, Value::Null)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", "/api/fs/tree", Some(&cookie), Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    std::fs::write(
        root.join("session.jsonl"),
        format!("{FIRST_TURN}{FIRST_TURN}"),
    )
    .unwrap();
    tokio::time::sleep(CACHE_TTL + Duration::from_millis(20)).await;
    assert_eq!(
        body(request(&app, "GET", &public_path, None, Value::Null).await).await["turns"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let revoke_path = format!("/api/aow/session-shares/{}", share["id"].as_str().unwrap());
    assert_eq!(
        request(&app, "DELETE", &revoke_path, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "DELETE", &revoke_path, Some(&new_cookie), Value::Null)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    // Cached content is unavailable immediately after revocation, including after restart.
    assert_eq!(
        request(&app, "GET", &public_path, None, Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert!(state(&root).session_shares.lookup(token).is_err());
    cache_locator(&restarted, locator(&root));
    let next_share =
        body(request(&app, "POST", create_path, Some(&new_cookie), payload).await).await;
    assert_ne!(next_share["path"], share["path"]);
}

#[tokio::test]
async fn shared_transcripts_keep_trusted_root_checks_and_hide_internal_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let shares = SessionShares::in_memory();
    let info = shares.create(locator(root)).unwrap();
    let token = info.path.strip_prefix("/share/").unwrap();
    let error = shares.read(token).await.err().unwrap().into_response();
    assert_eq!(error.status(), StatusCode::GONE);
    assert!(
        !body(error)
            .await
            .to_string()
            .contains(root.to_str().unwrap())
    );

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.jsonl"), FIRST_TURN).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.jsonl"),
        root.join("session.jsonl"),
    )
    .unwrap();
    assert_eq!(
        shares
            .read(token)
            .await
            .err()
            .unwrap()
            .into_response()
            .status(),
        StatusCode::GONE
    );
}

#[tokio::test]
async fn a_revoked_share_waiting_for_another_reader_cannot_return_cached_content() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("session.jsonl"), FIRST_TURN).unwrap();
    let shares = SessionShares::in_memory();
    let info = shares.create(locator(dir.path())).unwrap();
    let token = info.path.strip_prefix("/share/").unwrap();
    shares.read(token).await.unwrap();
    let entry = shares.lookup(token).unwrap();
    let guard = entry.snapshot.lock().await;
    let read = shares.read(token);
    tokio::pin!(read);
    tokio::select! {
        _ = &mut read => panic!("read should wait for the cache lock"),
        _ = tokio::task::yield_now() => {}
    }
    shares.revoke(&info.id).unwrap();
    drop(guard);
    assert_eq!(
        read.await.err().unwrap().into_response().status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn a_failed_save_does_not_publish_a_share() {
    let dir = tempfile::tempdir().unwrap();
    let shares = SessionShares::persistent(dir.path()).unwrap();
    std::fs::create_dir(dir.path().join(SHARES_FILE)).unwrap();
    assert!(shares.create(locator(dir.path())).is_err());
    assert!(shares.find("codex", "test-session").unwrap().is_none());
}
