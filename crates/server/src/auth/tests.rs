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
