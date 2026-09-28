use super::*;
use crate::{
    AppState, BasePath, HttpError,
    auth::{
        AuthService,
        provider::{LoginProvider, VerifiedIdentity},
    },
    operations::OperationService,
};
use aow_operation_log::{ReadOptions, Reader, Record};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

struct LocalProvider;
impl LoginProvider for LocalProvider {
    fn id(&self) -> &'static str {
        "local-test"
    }
    fn label(&self) -> &'static str {
        "Local test"
    }
    fn check_configured(&self) -> Result<(), HttpError> {
        Ok(())
    }
    fn authenticate(&self, credentials: &Value) -> Result<VerifiedIdentity, HttpError> {
        if credentials["username"] == "admin" && credentials["password"] == "secret-password" {
            Ok(VerifiedIdentity {
                subject: "admin".into(),
                revision: "1".into(),
            })
        } else {
            Err(super::super::authentication_required())
        }
    }
    fn validate_session(&self, _: &VerifiedIdentity) -> Result<bool, HttpError> {
        Ok(true)
    }
}

fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<head></head>login").unwrap();
    let mut state =
        AppState::new(dir.path().to_owned()).with_base_path(BasePath::parse("/tools/aow").unwrap());
    state.auth = AuthService::with_providers(vec![Arc::new(LocalProvider)]);
    state.operations = OperationService::persistent(&dir.path().join("logs")).unwrap();
    (dir, state)
}

fn records(dir: &std::path::Path) -> Vec<Record> {
    Reader::new(dir.join("logs"))
        .read(&ReadOptions::default())
        .unwrap()
        .items
}

fn login_request(body: String) -> Request<Body> {
    Request::post("/tools/aow/api/auth/login?password=query-secret")
        .header("content-type", "application/json")
        .header("user-agent", "Audit test browser")
        .header("host", "aow.example")
        .header("cookie", "aow_session=secret-cookie")
        .header("authorization", "Bearer secret-bearer")
        .header("x-forwarded-for", "198.51.100.123")
        .header("x-real-ip", "198.51.100.124")
        .header("forwarded", "for=198.51.100.125;proto=https")
        .header(
            "referer",
            "https://user:referer-password@aow.example/aow/?token=referer-secret#fragment-secret",
        )
        .extension(ConnectInfo(
            "192.0.2.10:45678".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap()
}

fn valid_body() -> String {
    json!({"method":"local-test", "username":"admin", "password":"secret-password"}).to_string()
}

#[tokio::test]
async fn all_login_outcomes_are_persisted_with_peer_evidence_and_without_secrets() {
    for (body, origin, limited, expected, reason) in [
        (valid_body(), None, false, StatusCode::OK, "login_succeeded"),
        (
            valid_body().replace("secret-password", "wrong-password"),
            None,
            false,
            StatusCode::UNAUTHORIZED,
            "authentication_required",
        ),
        (
            "{broken secret-password".into(),
            None,
            false,
            StatusCode::BAD_REQUEST,
            "invalid_login_request",
        ),
        (
            valid_body().replace("local-test", "unknown"),
            None,
            false,
            StatusCode::BAD_REQUEST,
            "unsupported_login_method",
        ),
        (
            valid_body(),
            Some("https://evil.example"),
            false,
            StatusCode::FORBIDDEN,
            "invalid_origin",
        ),
        (
            valid_body(),
            None,
            true,
            StatusCode::TOO_MANY_REQUESTS,
            "login_rate_limited",
        ),
        (
            "x".repeat(2 * 1024 * 1024 + 1),
            None,
            false,
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_login_request",
        ),
    ] {
        let (dir, state) = fixture();
        let _permits = limited.then(|| {
            (
                state.auth.limiter.begin().unwrap(),
                state.auth.limiter.begin().unwrap(),
            )
        });
        let mut request = login_request(body);
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("origin", origin.parse().unwrap());
        }
        let response = crate::build_router(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected);
        let logs = records(dir.path());
        assert_eq!(logs.len(), 1);
        let record = &logs[0];
        assert_eq!(record.kind, "auth.login");
        assert_eq!(
            record.event,
            if expected.is_success() {
                "login_succeeded"
            } else {
                "login_failed"
            }
        );
        assert_eq!(record.source, "auth");
        let data: Value = serde_json::from_str(&record.message).unwrap();
        assert_eq!(data["status"], expected.as_u16());
        assert_eq!(data["reason"], reason);
        assert_eq!(data["request"]["peer_ip"], "192.0.2.10");
        assert_eq!(data["request"]["peer_port"], 45678);
        assert_eq!(data["request"]["path"], "/tools/aow/api/auth/login");
        assert_eq!(
            data["request"]["x_forwarded_for_unverified"],
            "198.51.100.123"
        );
        assert_eq!(data["request"]["x_real_ip_unverified"], "198.51.100.124");
        assert_eq!(data["request"]["user_agent"], "Audit test browser");
        assert_eq!(
            data["request"]["referer_unverified"],
            "https://aow.example/aow/"
        );
        assert_eq!(
            data["subject"],
            if expected.is_success() {
                json!("admin")
            } else {
                Value::Null
            }
        );
        for secret in [
            "secret-password",
            "wrong-password",
            "secret-cookie",
            "secret-bearer",
            "query-secret",
            "referer-secret",
            "referer-password",
            "fragment-secret",
        ] {
            assert!(!record.message.contains(secret), "leaked {secret}");
        }
        uuid::Uuid::parse_str(&record.operation_id).unwrap();
        chrono::DateTime::parse_from_rfc3339(&record.timestamp).unwrap();
    }
}

#[tokio::test]
async fn anonymous_api_and_page_visits_are_audited_but_public_assets_and_authenticated_visits_are_not()
 {
    let (dir, state) = fixture();
    let app = crate::build_router(state.clone());
    for path in [
        "/api/fs/tree",
        "/fs/private-file",
        "/help",
        "/aow/tabs/terminal/123",
        "/m",
        "/index.html",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/tools/aow{path}?token=secret-query"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if path.starts_with("/api") || path.starts_with("/fs") || path == "/help" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::OK
            }
        );
    }
    let logs = records(dir.path());
    assert_eq!(logs.len(), 6);
    assert_eq!(
        logs.iter()
            .filter(|log| log.event == "access_denied")
            .count(),
        3
    );
    assert_eq!(
        logs.iter()
            .filter(|log| log.event == "anonymous_page_access")
            .count(),
        3
    );
    assert!(
        logs.iter()
            .all(|log| log.kind == "auth.access" && !log.message.contains("secret-query"))
    );
    for path in [
        "/api/health",
        "/api/auth/status",
        "/assets/app.js",
        "/favicon.ico",
        "/share/public-token",
        "/api/public/session-shares/public-token",
    ] {
        app.clone()
            .oneshot(
                Request::get(format!("/tools/aow{path}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    let token = state
        .auth
        .login(
            "local-test",
            json!({"username":"admin", "password":"secret-password"}),
        )
        .unwrap();
    for path in ["/aow/", "/api/fs/tree"] {
        app.clone()
            .oneshot(
                Request::get(format!("/tools/aow{path}"))
                    .header(
                        "cookie",
                        format!("{}={token}", state.base_path.cookie_name()),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    assert_eq!(records(dir.path()).len(), 6);
    // A forged session is still anonymous, and the secret is never persisted.
    app.oneshot(
        Request::get("/tools/aow/api/fs/tree")
            .header(
                "cookie",
                format!("{}=forged-secret", state.base_path.cookie_name()),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    let logs = records(dir.path());
    assert_eq!(logs.len(), 7);
    let data: Value = serde_json::from_str(&logs[0].message).unwrap();
    assert_eq!(data["request"]["session_cookie_present"], true);
    assert!(data["request"]["peer_ip"].is_null());
    assert!(!logs[0].message.contains("forged-secret"));
}

#[test]
fn evidence_bounds_untrusted_fields_and_redacts_share_tokens_and_url_credentials() {
    assert_eq!(
        safe_url("https://user:password@host/share/bearer-token?secret=yes#secret").unwrap(),
        "https://host/share/[redacted]"
    );
    assert_eq!(
        safe_path("/tools/aow/api/public/session-shares/bearer-token"),
        "/tools/aow/api/public/session-shares/[redacted]"
    );
    assert_eq!(clean("a\r\nb", 10), "a��b");
    assert_eq!(clean(&"汉".repeat(10000), 256).chars().count(), 257);
}

#[tokio::test]
async fn socket_peer_is_recorded_independently_of_spoofed_proxy_headers() {
    let (dir, state) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            crate::build_router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/tools/aow/api/fs/tree"))
        .header("x-forwarded-for", "198.51.100.99")
        .send()
        .await;
    server.abort();
    assert_eq!(response.unwrap().status(), StatusCode::UNAUTHORIZED);
    let logs = records(dir.path());
    let data: Value = serde_json::from_str(&logs[0].message).unwrap();
    assert_eq!(data["request"]["peer_ip"], "127.0.0.1");
    assert!(data["request"]["peer_port"].as_u64().unwrap() > 0);
    assert_eq!(
        data["request"]["x_forwarded_for_unverified"],
        "198.51.100.99"
    );
}

struct SlowProvider {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl LoginProvider for SlowProvider {
    fn id(&self) -> &'static str {
        "local-test"
    }
    fn label(&self) -> &'static str {
        "Slow test"
    }
    fn check_configured(&self) -> Result<(), HttpError> {
        Ok(())
    }
    fn authenticate(&self, credentials: &Value) -> Result<VerifiedIdentity, HttpError> {
        self.started.notify_one();
        tokio::runtime::Handle::current().block_on(self.release.notified());
        LocalProvider.authenticate(credentials)
    }
    fn validate_session(&self, _: &VerifiedIdentity) -> Result<bool, HttpError> {
        Ok(true)
    }
}

#[tokio::test]
async fn admitted_login_finishes_auditing_when_the_client_disconnects_during_verification() {
    let (dir, mut state) = fixture();
    let provider = Arc::new(SlowProvider {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    state.auth = AuthService::with_providers(vec![provider.clone()]);
    let app = crate::build_router(state);
    let caller = tokio::spawn(app.oneshot(login_request(valid_body())));
    provider.started.notified().await;
    caller.abort();
    provider.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let logs = records(dir.path());
            if !logs.is_empty() {
                assert_eq!(logs.len(), 1);
                assert_eq!(logs[0].event, "login_succeeded");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
