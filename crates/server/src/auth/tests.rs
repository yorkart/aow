use std::{
    fs,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use serde_json::json;

use super::provider::VerifiedIdentity;
use super::*;

fn password_login(auth: &AuthService, username: &str, password: &str) -> Result<String, HttpError> {
    auth.login(
        "password",
        json!({ "username": username, "password": password }),
    )
}

#[test]
fn account_and_password_are_both_required_and_password_is_not_normalized() {
    let directory = tempfile::tempdir().unwrap();
    let password = " 密码 p@ss ";
    write_credentials(directory.path(), "本地用户", password);
    let auth = AuthService::persistent(directory.path());
    assert!(password_login(&auth, "本地用户", password).is_ok());
    for (username, password) in [
        ("", password),
        (" 本地用户", password),
        ("other", password),
        ("本地用户", ""),
        ("本地用户", "123456"),
        ("本地用户", password.trim()),
    ] {
        assert!(password_login(&auth, username, password).is_err());
    }
}

#[test]
fn changed_account_or_password_invalidates_existing_sessions() {
    let directory = tempfile::tempdir().unwrap();
    write_credentials(directory.path(), "admin", "password");
    let auth = AuthService::persistent(directory.path());
    let token = password_login(&auth, "admin", "password").unwrap();
    assert!(auth.authorize(Some(&token)).is_ok());
    write_credentials(directory.path(), "admin", "new-password");
    assert!(auth.authorize(Some(&token)).is_err());
    let token = password_login(&auth, "admin", "new-password").unwrap();
    assert!(auth.authorize(Some(&token)).is_ok());

    // Renaming an account alone must revoke its sessions, even with the same hash.
    let path = directory.path().join(password::CREDENTIALS_FILE);
    let mut credentials: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    credentials["username"] = json!("renamed");
    fs::write(path, serde_json::to_vec(&credentials).unwrap()).unwrap();
    assert!(auth.authorize(Some(&token)).is_err());
    assert!(password_login(&auth, "admin", "new-password").is_err());
    assert!(password_login(&auth, "renamed", "new-password").is_ok());
}

#[test]
fn missing_legacy_and_malformed_credentials_never_allow_access() {
    let directory = tempfile::tempdir().unwrap();
    let auth = AuthService::persistent(directory.path());
    fs::write(
        directory.path().join("pin.md5"),
        "e10adc3949ba59abbe56e057f20f883e",
    )
    .unwrap();
    assert!(!auth.status(None).configured);
    assert!(auth.authorize(None).is_err());
    assert!(password_login(&auth, "admin", "123456").is_err());
    for contents in [
        "",
        "{}",
        "not JSON",
        r#"{"version":1,"username":"admin","salt":"invalid","password_hash":"invalid"}"#,
    ] {
        fs::write(directory.path().join(password::CREDENTIALS_FILE), contents).unwrap();
        assert!(!auth.status(None).configured);
        assert!(auth.authorize(None).is_err());
        assert!(password_login(&auth, "admin", "123456").is_err());
    }
}

struct TestProvider {
    id: &'static str,
    revision: AtomicU64,
    enabled: AtomicBool,
}

impl TestProvider {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            revision: AtomicU64::new(1),
            enabled: AtomicBool::new(true),
        }
    }

    fn identity(&self) -> VerifiedIdentity {
        VerifiedIdentity {
            subject: "same-user".into(),
            revision: self.revision.load(Ordering::SeqCst).to_string(),
        }
    }
}

impl LoginProvider for TestProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn label(&self) -> &'static str {
        self.id
    }
    fn check_configured(&self) -> Result<(), HttpError> {
        if self.enabled.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(HttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "test_not_configured",
                "Test login unavailable",
                None,
            ))
        }
    }
    fn authenticate(&self, credentials: &Value) -> Result<VerifiedIdentity, HttpError> {
        self.check_configured()?;
        if credentials["proof"] != "accepted" {
            return Err(authentication_required());
        }
        Ok(self.identity())
    }
    fn validate_session(&self, identity: &VerifiedIdentity) -> Result<bool, HttpError> {
        self.check_configured()?;
        Ok(*identity == self.identity())
    }
}

#[test]
fn providers_share_sessions_but_never_accept_each_others_identity() {
    let first = Arc::new(TestProvider::new("first"));
    let second = Arc::new(TestProvider::new("second"));
    let auth = AuthService::with_providers(vec![first.clone(), second.clone()]);
    let token = auth.login("first", json!({ "proof": "accepted" })).unwrap();
    let other_token = auth
        .login("second", json!({ "proof": "accepted" }))
        .unwrap();
    assert!(auth.authorize(Some(&token)).is_ok());
    assert!(auth.authorize(Some(&other_token)).is_ok());
    assert!(
        auth.login("unknown", json!({ "proof": "accepted" }))
            .is_err()
    );
    assert!(auth.login("first", json!({ "proof": "wrong" })).is_err());
    assert!(auth.authorize(Some("unissued-token")).is_err());
    second.revision.store(2, Ordering::SeqCst);
    assert!(auth.authorize(Some(&token)).is_ok());
    assert!(auth.authorize(Some(&other_token)).is_err());
    // The second provider still has exactly the first provider's old identity.
    // Removing the first must not let its token fall back to the second.
    second.revision.store(1, Ordering::SeqCst);
    let without_first = AuthService {
        providers: vec![second.clone()],
        ..auth.clone()
    };
    assert!(without_first.authorize(Some(&token)).is_err());
    first.enabled.store(false, Ordering::SeqCst);
    let status = auth.status(None);
    assert!(status.configured);
    assert!(!status.authenticated);
    assert!(!status.methods[0].configured);
    assert!(status.methods[1].configured);
    assert!(AuthService::with_providers(vec![]).authorize(None).is_err());
}

#[tokio::test]
async fn another_login_method_uses_the_same_http_cookie_and_access_policy() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    let root = tempfile::tempdir().unwrap();
    let mut state = crate::AppState::new(root.path().to_path_buf());
    state.auth = AuthService::with_providers(vec![Arc::new(TestProvider::new("test-method"))]);
    let app = crate::build_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"method":"test-method","proof":"accepted"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"].clone();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["methods"][0]["id"], "test-method");
    for (authenticated, expected) in [(false, StatusCode::UNAUTHORIZED), (true, StatusCode::OK)] {
        let mut request = Request::get("/api/fs/tree");
        if authenticated {
            request = request.header("cookie", cookie.clone());
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn session_access_is_revoked_on_logout_rotation_or_provider_failure() {
    use std::time::Duration;
    for failed_configuration in [false, true] {
        let provider = Arc::new(TestProvider::new("test"));
        let auth = AuthService::with_providers(vec![provider.clone()]);
        let token = auth.login("test", json!({"proof": "accepted"})).unwrap();
        let access = auth.access(Some(&token)).unwrap().unwrap();
        if failed_configuration {
            provider.enabled.store(false, Ordering::SeqCst);
        } else {
            provider.revision.store(2, Ordering::SeqCst);
        }
        assert!(access.validate(true).is_err());
        tokio::time::timeout(Duration::from_millis(100), access.revoked())
            .await
            .unwrap();
        // Restoring the old credentials cannot resurrect a revoked session.
        provider.enabled.store(true, Ordering::SeqCst);
        provider.revision.store(1, Ordering::SeqCst);
        assert!(access.validate(true).is_err());
    }
}

#[tokio::test]
async fn logout_revokes_http_and_stream_access_and_clears_the_scoped_secure_cookie() {
    use axum::{body::Body, http::Request};
    use futures_util::StreamExt;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let mut state = crate::AppState::new(root.path().to_owned())
        .with_base_path(crate::BasePath::parse("/tools/aow").unwrap());
    state.auth = AuthService::with_providers(vec![Arc::new(TestProvider::new("test"))]);
    state = state.with_secure_cookies(true);
    let auth = state.auth.clone();
    let app = crate::build_router(state);
    let login = app
        .clone()
        .oneshot(
            Request::post("/tools/aow/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"method":"test","proof":"accepted"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    assert_eq!(login.headers()["cache-control"], "no-store");
    let cookie = login.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("Path=/tools/aow/; HttpOnly; SameSite=Strict; Secure"));
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let token = cookie.split_once('=').unwrap().1;
    let access = auth.access(Some(token)).unwrap().unwrap();
    let events = app
        .clone()
        .oneshot(
            Request::get("/tools/aow/api/operation-logs")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(events.status(), StatusCode::OK);
    let events = app
        .clone()
        .oneshot(
            Request::get("/tools/aow/api/operations/stream")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(events.status(), StatusCode::OK);
    let mut stream = events.into_body().into_data_stream();
    assert!(stream.next().await.is_some());
    let logout = app
        .clone()
        .oneshot(
            Request::post("/tools/aow/api/auth/logout")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(
        logout.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Path=/tools/aow/; HttpOnly; SameSite=Strict; Secure; Max-Age=0")
    );
    assert!(access.validate(true).is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .is_none()
    );
    let denied = app
        .oneshot(
            Request::get("/tools/aow/api/fs/tree")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_admission_rejects_excess_work_and_ignores_forged_forwarded_headers() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let mut state = crate::AppState::new(root.path().to_owned());
    state.auth = AuthService::with_providers(vec![Arc::new(TestProvider::new("test"))]);
    let auth = state.auth.clone();
    let app = crate::build_router(state);
    let first = auth.limiter.begin().unwrap();
    let second = auth.limiter.begin().unwrap();
    let make_login = || {
        Request::post("/api/auth/login")
            .header("content-type", "application/json")
            .header("x-forwarded-for", "192.0.2.1")
            .header("x-forwarded-proto", "https")
            .body(Body::from(r#"{"method":"test","proof":"accepted"}"#))
            .unwrap()
    };
    let limited = app.clone().oneshot(make_login()).await.unwrap();
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().contains_key("retry-after"));
    assert!(!limited.headers().contains_key("set-cookie"));
    drop(first);
    drop(second);
    let login = app.clone().oneshot(make_login()).await.unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    assert!(
        !login.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("; Secure")
    );
    for path in ["/api/auth/login", "/api/auth/logout"] {
        let denied = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("host", "aow.example")
                    .header("origin", "https://evil.example")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"method":"test","proof":"accepted"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
}
