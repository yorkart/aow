use super::*;
use crate::terminal::notifications::TaskStopSource;
use aow_automations::FailureNotification;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

fn wechat_credentials() -> aow_im::wechat::Credentials {
    aow_im::wechat::Credentials {
        account_id: "wechat-bot".into(),
        user_id: "scanner".into(),
        bot_token: "wechat-private-token".into(),
        base_url: "https://ilinkai.weixin.qq.com".into(),
        binding_id: "00000000-0000-4000-8000-000000000001".into(),
    }
}

#[test]
fn wechat_binding_persists_privately_and_provider_edits_preserve_each_other() {
    let root = tempfile::tempdir().unwrap();
    let manager = NotificationManager::persistent(root.path()).unwrap();
    manager
        .update(im("cli_test", Some("feishu-secret")))
        .unwrap();
    manager
        .update(SettingsUpdate::WechatBinding {
            credentials: wechat_credentials(),
        })
        .unwrap();
    // A legacy Feishu-only client cannot erase a newly bound WeChat account.
    manager.update(im("cli_test", None)).unwrap();
    assert!(manager.wechat().is_some());
    manager
        .update(preferences(
            true,
            vec![Channel::Page, Channel::Wechat, Channel::Feishu],
        ))
        .unwrap();
    assert!(
        manager
            .update(SettingsUpdate::ImRemove {
                provider: ImKind::Wechat
            })
            .is_err()
    );
    let view = serde_json::to_string(&manager.view()).unwrap();
    assert!(view.contains("scanner"));
    for secret in [
        "bot_token",
        "wechat-private-token",
        "feishu-secret",
        "binding_id",
    ] {
        assert!(!view.contains(secret));
    }
    drop(manager);
    let restored = NotificationManager::persistent(root.path()).unwrap();
    assert_eq!(
        restored.wechat_credentials().unwrap().bot_token,
        "wechat-private-token"
    );
    assert_eq!(serde_json::to_string(&restored.view()).unwrap(), view);
    restored
        .update(preferences(true, vec![Channel::Page]))
        .unwrap();
    restored
        .update(SettingsUpdate::ImRemove {
            provider: ImKind::Feishu,
        })
        .unwrap();
    assert!(restored.wechat().is_some());
    restored
        .update(im("cli_test", Some("other-secret")))
        .unwrap();
    restored
        .update(SettingsUpdate::ImRemove {
            provider: ImKind::Wechat,
        })
        .unwrap();
    assert!(restored.wechat().is_none());
    assert_eq!(
        restored
            .inner
            .state
            .lock()
            .unwrap()
            .document
            .im
            .providers
            .len(),
        1
    );
}

#[test]
fn automation_routes_to_snapshot_channel_and_uses_transport_independent_messages() {
    let manager = NotificationManager::in_memory();
    manager
        .update(SettingsUpdate::WechatBinding {
            credentials: wechat_credentials(),
        })
        .unwrap();
    let run: aow_automations::Run = serde_json::from_value(json!({
        "id":"run-1", "task_id":"task-1", "task_revision":1, "task_name":"检查 <at id=all>", "agent":"codex",
        "source":"scheduled", "status":"failed", "started_at":"2026-01-01T00:00:00Z", "message":"失败原因"
    })).unwrap();
    assert!(
        manager
            .automation_delivery(run.clone(), FailureNotification::Feishu)
            .is_none()
    );
    let (provider, event) = manager
        .automation_delivery(run, FailureNotification::Wechat)
        .unwrap();
    assert!(matches!(provider, Provider::Wechat(_)));
    let message = messages::automation_failure(&event);
    assert!(message.error && !message.markdown);
    assert_eq!(message.title, "自动化失败 · 检查 <at id=all>");
    assert!(message.text(4000).contains("执行 ID：run-1"));
    let message = messages::task_completed(&super::tests::event());
    assert_eq!(message.title, "AOW·task·TraeCode CLI·完成");
    assert!(message.text(4000).contains("Session ID：session-1"));
}

#[tokio::test]
async fn automation_delivery_uses_local_bot_and_its_own_preference_and_link() {
    let manager = NotificationManager::in_memory();
    let run: aow_automations::Run = serde_json::from_value(json!({
        "id":"run/one", "task_id":"task/one", "task_revision":1, "task_name":"检查", "agent":"codex",
        "source":"scheduled", "status":"failed", "started_at":"2026-01-01T00:00:00Z"
    })).unwrap();
    assert!(
        !manager
            .send_automation_failure(run.clone(), FailureNotification::Feishu, "delivery")
            .await
            .unwrap()
    );
    manager
        .update(im("cli_test", Some("private-secret")))
        .unwrap();
    manager
        .update(SettingsUpdate::Notifications {
            agent_task_completed: TaskCompletedSettings {
                enabled: false,
                channels: vec![],
            },
            public_base_url: Some("https://aow.example.com".into()),
        })
        .unwrap();
    let (_, event) = manager
        .automation_delivery(run.clone(), FailureNotification::Feishu)
        .expect("automation ignores interactive completion switch");
    assert_eq!(
        event.run_url.as_deref(),
        Some("https://aow.example.com/aow/tabs/automation/task%2Fone/runs/run%2Fone")
    );
    manager
        .update(SettingsUpdate::Notifications {
            agent_task_completed: TaskCompletedSettings::default(),
            public_base_url: Some("https://aow.example.com/tools/aow/".into()),
        })
        .unwrap();
    assert_eq!(
        manager
            .automation_delivery(run.clone(), FailureNotification::Feishu)
            .unwrap()
            .1
            .run_url
            .as_deref(),
        Some("https://aow.example.com/tools/aow/aow/tabs/automation/task%2Fone/runs/run%2Fone")
    );
    manager
        .update(SettingsUpdate::Im { providers: vec![] })
        .unwrap();
    assert!(
        manager
            .automation_delivery(run, FailureNotification::Feishu)
            .is_none()
    );
}

fn im(app: &str, secret: Option<&str>) -> SettingsUpdate {
    SettingsUpdate::Im {
        providers: vec![ImConfigUpdate::Feishu {
            app_id: app.into(),
            app_secret: secret.map(str::to_owned),
        }],
    }
}
fn preferences(enabled: bool, channels: Vec<Channel>) -> SettingsUpdate {
    SettingsUpdate::Notifications {
        agent_task_completed: TaskCompletedSettings { enabled, channels },
        public_base_url: None,
    }
}
fn event() -> TaskStopNotification {
    TaskStopNotification {
        agent: "traecli".into(),
        session_id: "session-1".into(),
        title: "task".into(),
        cwd: "/repo".into(),
        turn_id: None,
        conclusion: Some("本轮结论".into()),
        instance_ids: vec!["pane".into()],
        sources: vec![TaskStopSource {
            project_name: "AOW".into(),
            tab_name: "开发".into(),
            workspace_root: "/repo".into(),
            tab_id: "tab".into(),
            tab_url: None,
        }],
    }
}

#[test]
fn notification_urls_persist_validate_and_support_legacy_settings() {
    let root = tempfile::tempdir().unwrap();
    let mut legacy = serde_json::to_value(Document::default()).unwrap();
    legacy["notifications"]
        .as_object_mut()
        .unwrap()
        .remove("public_base_url");
    std::fs::write(
        root.path().join(FILE_NAME),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let manager = NotificationManager::persistent(root.path()).unwrap();
    let (page, mut receiver) = broadcast::channel(4);
    manager.dispatch(event(), &page);
    assert_eq!(receiver.try_recv().unwrap().sources[0].tab_url, None);
    let update = |url: &str| SettingsUpdate::Notifications {
        agent_task_completed: TaskCompletedSettings::default(),
        public_base_url: Some(url.into()),
    };
    manager
        .update(update(" https://aow.example.com/ "))
        .unwrap();
    for invalid in [
        "javascript:alert(1)",
        "//example.com",
        "https://user:secret@example.com",
        "https://example.com/bad%2fpath",
        "https://example.com/?x=1",
        "https://example.com/#x",
    ] {
        assert!(
            manager.update(update(invalid)).is_err(),
            "accepted {invalid}"
        );
    }
    manager
        .update(preferences(true, vec![Channel::Page]))
        .unwrap();
    let restored = NotificationManager::persistent(root.path()).unwrap();
    assert_eq!(
        restored.view().notifications.public_base_url,
        "https://aow.example.com"
    );
    let mut linked_event = event();
    linked_event.sources[0].tab_id = "tab/中文?#".into();
    restored.dispatch(linked_event, &page);
    assert_eq!(
        receiver.try_recv().unwrap().sources[0].tab_url.as_deref(),
        Some("https://aow.example.com/aow/tabs/terminal/tab%2F%E4%B8%AD%E6%96%87%3F%23")
    );
    restored
        .update(update("https://aow.example.com:8443/tools/aow/"))
        .unwrap();
    let restored = NotificationManager::persistent(root.path()).unwrap();
    assert_eq!(
        restored.view().notifications.public_base_url,
        "https://aow.example.com:8443/tools/aow"
    );
    restored.dispatch(event(), &page);
    assert_eq!(
        receiver.try_recv().unwrap().sources[0].tab_url.as_deref(),
        Some("https://aow.example.com:8443/tools/aow/aow/tabs/terminal/tab")
    );
    restored.update(update("")).unwrap();
    restored.dispatch(event(), &page);
    assert_eq!(receiver.try_recv().unwrap().sources[0].tab_url, None);
}

#[test]
fn settings_persist_privately_and_public_views_never_return_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let manager = NotificationManager::persistent(directory.path()).unwrap();
    manager
        .update(im("cli_test", Some("private-secret")))
        .unwrap();
    manager
        .update(preferences(true, vec![Channel::Feishu]))
        .unwrap();
    let view = serde_json::to_string(&manager.view()).unwrap();
    assert!(!view.contains("private-secret"));
    assert!(!view.contains("app_secret"));
    assert!(view.contains("secret_configured"));
    let path = directory.path().join(FILE_NAME);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!directory.path().join("config-repo").exists());
    let restored = NotificationManager::persistent(directory.path()).unwrap();
    assert_eq!(serde_json::to_string(&restored.view()).unwrap(), view);
    restored.update(im("cli_test", None)).unwrap();
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("private-secret")
    );
}

#[test]
fn validates_enabled_channels_and_preserves_previous_settings_after_failed_updates() {
    let manager = NotificationManager::in_memory();
    assert!(manager.update(preferences(true, vec![])).is_err());
    assert!(
        manager
            .update(preferences(true, vec![Channel::Feishu]))
            .is_err()
    );
    manager.update(im("cli_test", Some("secret"))).unwrap();
    manager
        .update(preferences(true, vec![Channel::Feishu]))
        .unwrap();
    assert!(
        manager
            .update(SettingsUpdate::Im { providers: vec![] })
            .is_err()
    );
    assert!(manager.update(im("cli_changed", None)).is_err());
    assert_eq!(
        manager
            .inner
            .state
            .lock()
            .unwrap()
            .document
            .im
            .providers
            .len(),
        1
    );
    manager.update(preferences(false, vec![])).unwrap();
    manager
        .update(SettingsUpdate::Im { providers: vec![] })
        .unwrap();
}

#[test]
fn changing_credentials_replaces_provider_but_notification_changes_preserve_token_cache() {
    let manager = NotificationManager::in_memory();
    manager.update(im("cli_test", Some("first"))).unwrap();
    let original = manager.inner.state.lock().unwrap().providers[0]
        .provider
        .clone();
    manager
        .update(preferences(true, vec![Channel::Page, Channel::Feishu]))
        .unwrap();
    let preserved = manager.inner.state.lock().unwrap().providers[0]
        .provider
        .clone();
    manager.update(im("cli_test", Some("second"))).unwrap();
    let changed = manager.inner.state.lock().unwrap().providers[0]
        .provider
        .clone();
    let (Provider::Feishu(original), Provider::Feishu(preserved), Provider::Feishu(changed)) =
        (original, preserved, changed)
    else {
        panic!("expected Feishu providers")
    };
    assert!(Arc::ptr_eq(&original, &preserved));
    assert!(!Arc::ptr_eq(&original, &changed));
}

#[test]
fn dispatch_obeys_channels_without_browser_or_session_deduplication() {
    let manager = NotificationManager::in_memory();
    let (page, mut receiver) = broadcast::channel(16);
    manager.dispatch(event(), &page);
    manager.dispatch(event(), &page);
    assert_eq!(receiver.try_recv().unwrap().session_id, "session-1");
    assert!(receiver.try_recv().is_ok());
    manager.update(preferences(false, vec![])).unwrap();
    manager.dispatch(event(), &page);
    assert!(receiver.try_recv().is_err());
    manager.update(im("cli_test", Some("secret"))).unwrap();
    manager
        .update(preferences(true, vec![Channel::Feishu]))
        .unwrap();
    drop(receiver);
    manager.dispatch(event(), &page);
    manager.dispatch(event(), &page);
    let mut im_receiver = manager.inner.receiver.lock().unwrap().take().unwrap();
    let first = im_receiver.try_recv().unwrap();
    let second = im_receiver.try_recv().unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.event.sources[0].project_name, "AOW");
    assert_eq!(first.event.sources[0].tab_name, "开发");
    assert_eq!(first.event.conclusion.as_deref(), Some("本轮结论"));
    assert_eq!(first.event.session_id, "session-1");
    manager.update(preferences(false, vec![])).unwrap();
    assert_ne!(first.revision, manager.inner.state.lock().unwrap().revision);
}

#[tokio::test]
async fn settings_api_redacts_secrets_and_rejects_invalid_preferences() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let app = crate::build_router(AppState::new(PathBuf::from("/nonexistent")));
    for (body, status) in [
        (
            json!({"section":"im","providers":[{"provider":"feishu","app_id":"cli_test","app_secret":"private-secret"}]}),
            StatusCode::OK,
        ),
        (
            json!({"section":"notifications","agent_task_completed":{"enabled":true,"channels":[]}}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/aow/notification-settings")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let bytes = to_bytes(response.into_body(), 10000).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-secret"));
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/aow/notification-settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap()).unwrap();
    assert_eq!(body["im"]["providers"][0]["secret_configured"], true);
    assert!(body["im"]["providers"][0].get("app_secret").is_none());
    assert_eq!(
        body["notifications"]["agent_task_completed"]["channels"],
        json!(["page"])
    );
}
