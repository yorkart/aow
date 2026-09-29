use super::*;
use crate::{
    AppState, auth,
    notifications::{SettingsUpdate, TaskCompletedSettings},
};
use aow_operation_log::{ReadOptions, Reader};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

#[tokio::test]
async fn login_notifies_all_providers_while_anonymous_access_only_logs() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("index.html"), "<head></head>login").unwrap();
    auth::write_credentials(directory.path(), "admin", "secret-password");
    let mut state = AppState::new(directory.path().to_owned());
    state.auth = auth::AuthService::persistent(directory.path());
    state.operations = OperationService::persistent(&directory.path().join("logs")).unwrap();
    let manager = state.aow.notifications();
    manager
        .update(SettingsUpdate::Im {
            providers: vec![aow_im::ImConfigUpdate::Feishu {
                app_id: "fake-app".into(),
                app_secret: Some("fake-secret".into()),
            }],
        })
        .unwrap();
    manager
        .update(SettingsUpdate::WechatBinding {
            credentials: aow_im::wechat::Credentials {
                account_id: "test-account".into(),
                user_id: "test-user".into(),
                bot_token: "fake-token".into(),
                base_url: "https://ilinkai.weixin.qq.com".into(),
                binding_id: "g123456789ab".into(),
            },
        })
        .unwrap();
    manager
        .update(SettingsUpdate::Notifications {
            agent_task_completed: TaskCompletedSettings {
                enabled: false,
                channels: vec![],
            },
            public_base_url: None,
        })
        .unwrap();
    // Capture the real dispatch queue; no network requests or real IM sends.
    let mut receiver = manager
        .inner
        .security
        .receiver
        .lock()
        .unwrap()
        .take()
        .unwrap();
    let app = crate::build_router(state.clone());
    let mut ids = std::collections::HashSet::new();
    for (password, expected) in [
        ("secret-password", StatusCode::OK),
        ("wrong-password", StatusCode::UNAUTHORIZED),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"username":"admin", "password":password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let delivery = receiver.try_recv().unwrap();
        assert_eq!(
            delivery
                .providers
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>(),
            [ImKind::Feishu, ImKind::Wechat]
        );
        assert_eq!(delivery.message.error, !expected.is_success());
        assert!(!delivery.message.markdown);
        let text = delivery.message.text(4000);
        for secret in [
            "secret-password",
            "wrong-password",
            "fake-secret",
            "fake-token",
        ] {
            assert!(!text.contains(secret));
        }
        assert!(ids.insert(delivery.record.operation_id));
    }
    for (path, expected) in [
        ("/api/fs/tree", StatusCode::UNAUTHORIZED),
        ("/aow/", StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(receiver.try_recv().is_err());
    }
    assert_eq!(ids.len(), 2);
    let logs = Reader::new(directory.path().join("logs"))
        .read(&ReadOptions::default())
        .unwrap()
        .items;
    assert_eq!(logs.len(), 4);
    assert_eq!(
        logs.iter()
            .filter(|record| record.kind == "auth.access")
            .count(),
        2
    );
    // Saturation must wait for room instead of silently dropping an event.
    let malformed_login = || {
        Request::post("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap()
    };
    for _ in 0..super::super::QUEUE_CAPACITY {
        app.clone().oneshot(malformed_login()).await.unwrap();
    }
    let pending = app.clone().oneshot(malformed_login());
    tokio::pin!(pending);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut pending)
            .await
            .is_err()
    );
    // A saturated login notification queue cannot delay anonymous access logs.
    let anonymous = app.oneshot(Request::get("/api/fs/tree").body(Body::empty()).unwrap());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), anonymous)
            .await
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    receiver.recv().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), &mut pending)
            .await
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(receiver.len(), super::super::QUEUE_CAPACITY);
}

struct MockProvider {
    calls: Arc<Mutex<Vec<String>>>,
    fail: bool,
}
impl ImProvider for MockProvider {
    async fn send(&self, _: &Message, delivery_id: &str) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(delivery_id.into());
        if self.fail {
            anyhow::bail!("upstream secret-token must never be logged");
        }
        Ok(())
    }
}

#[tokio::test]
async fn delivery_results_are_correlated_and_failures_do_not_suppress_other_providers_or_leak_errors()
 {
    let directory = tempfile::tempdir().unwrap();
    let operations = OperationService::persistent(directory.path()).unwrap();
    let record = Record {
        timestamp: chrono::Utc::now().to_rfc3339(),
        operation_id: "event-123".into(),
        boot_id: String::new(),
        kind: "auth.login".into(),
        source: "auth".into(),
        title: "登录成功".into(),
        event: "login_succeeded".into(),
        level: Level::Info,
        message: "{}".into(),
        project_id: None,
        resource: Some("/api/auth/login".into()),
        outcome: Some(Outcome::Succeeded),
        completed: None,
        total: None,
    };
    let message = Message {
        title: "登录成功".into(),
        fields: vec![],
        body_label: String::new(),
        body: String::new(),
        markdown: false,
        error: false,
    };
    let calls = Arc::new(Mutex::new(vec![]));
    let failed = MockProvider {
        calls: calls.clone(),
        fail: true,
    };
    let sent = MockProvider {
        calls: calls.clone(),
        fail: false,
    };
    tokio::join!(
        deliver(&failed, ImKind::Feishu, &message, &record, &operations),
        deliver(&sent, ImKind::Wechat, &message, &record, &operations)
    );
    assert_eq!(*calls.lock().unwrap(), ["event-123", "event-123"]);
    let records = Reader::new(directory.path())
        .read(&ReadOptions::default())
        .unwrap()
        .items;
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|r| r.outcome == Some(Outcome::Succeeded) && r.message.contains("wechat"))
    );
    assert!(
        records
            .iter()
            .any(|r| r.outcome == Some(Outcome::Failed) && r.message.contains("feishu"))
    );
    assert!(records.iter().all(|r| r.operation_id == "event-123"
        && r.kind == "auth.notification"
        && !r.message.contains("secret-token")));
}
