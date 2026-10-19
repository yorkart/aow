use super::*;
use crate::notifications::{Channel, SettingsUpdate};
use serde_json::json;

#[tokio::test]
async fn real_acp_prompts_reach_the_terminal_notification_subscription_without_a_view() {
    let root = tempfile::tempdir().unwrap();
    let state = crate::AppState::with_state_dir(
        root.path().into(),
        root.path().join("state"),
        root.path().join("missing.sock"),
    )
    .unwrap();
    let previous = state.aow.acp_settings().unwrap();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/agent.mjs");
    state
        .aow
        .save_acp_settings(
            json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[fixture]}}})
                .to_string(),
            previous.revision,
        )
        .await
        .unwrap();
    let connection = state.acp.connect("test", root.path().into()).await.unwrap();
    let session = state.acp.new_session(&connection.id).await.unwrap();
    let mut receiver = state.terminals.subscribe_task_stops();
    let mut turns = std::collections::HashSet::new();
    for _ in 0..2 {
        state
            .acp
            .start_prompt(
                &session.thread_id,
                serde_json::from_value(json!([{"type":"text","text":"message-ids"}])).unwrap(),
            )
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(10), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(turns.insert(event.turn_id.unwrap()));
        assert_eq!(
            event.sources[0].thread_id.as_deref(),
            Some(session.thread_id.as_str())
        );
        assert_eq!(
            event.conclusion.as_deref(),
            Some("ACP_LIVE_FOLLOWUP_VISIBLE")
        );
        assert_eq!(
            state.acp.snapshot(&session.thread_id).unwrap().status,
            "idle"
        );
        assert!(receiver.try_recv().is_err());
    }
    assert!(state.terminals.subscribe_task_stops().try_recv().is_err());
}

fn completion(turn_id: &str) -> TurnCompletion {
    TurnCompletion {
        session: aow_acp::SessionInfo {
            source: Default::default(),
            created_at: "2026-01-01T00:00:00Z".into(),
            parent: None,
            thread_id: "thread/中文".into(),
            session_id: Some("remote-session".into()),
            agent_id: "test-acp".into(),
            cwd: "/workspace/demo".into(),
            title: "ACP task".into(),
            status: "idle".into(),
            updated_at: String::new(),
            interacted_at: None,
        },
        turn_id: turn_id.into(),
        conclusion: Some("Finished this turn".into()),
        usage: Some(
            json!({"inputTokens":100,"outputTokens":20,"totalTokens":120,"cachedReadTokens":40}),
        ),
    }
}

#[tokio::test]
async fn acp_uses_shared_preferences_and_stable_thread_links_for_every_turn() {
    let manager = AowManager::in_memory();
    let (page, mut receiver) = broadcast::channel(8);
    let preferences = |enabled: bool, channels: Vec<Channel>| {
        serde_json::from_value::<SettingsUpdate>(json!({
        "section": "notifications", "agent_task_completed": { "enabled": enabled, "channels": channels },
        "public_base_url": "https://aow.example/tools",
    })).unwrap()
    };
    manager
        .notifications()
        .update(preferences(true, vec![Channel::Page]))
        .unwrap();
    for turn in ["first", "second"] {
        dispatch(&manager, &page, completion(turn)).await;
        let event = receiver.try_recv().unwrap();
        assert_eq!(event.turn_id.as_deref(), Some(turn));
        assert_eq!(event.session_id, "remote-session");
        assert_eq!(event.conclusion.as_deref(), Some("Finished this turn"));
        assert_eq!(event.usage.unwrap().total_tokens, 120);
        let source = &event.sources[0];
        assert_eq!(source.thread_id.as_deref(), Some("thread/中文"));
        assert!(source.tab_id.is_empty());
        assert_eq!(source.project_name, "demo");
        assert_eq!(
            source.tab_url.as_deref(),
            Some("https://aow.example/tools/aow/tabs/acp/thread%2F%E4%B8%AD%E6%96%87")
        );
    }
    assert!(receiver.try_recv().is_err());
    assert!(
        page.subscribe().try_recv().is_err(),
        "new subscribers never replay completions"
    );
    manager
        .notifications()
        .update(preferences(false, vec![Channel::Page]))
        .unwrap();
    dispatch(&manager, &page, completion("disabled")).await;
    manager.notifications().update(serde_json::from_value(json!({
        "section":"im", "providers":[{"provider":"feishu","app_id":"cli_test","app_secret":"test-secret"}],
    })).unwrap()).unwrap();
    // Do not start the delivery worker: this only exercises channel routing.
    manager
        .notifications()
        .update(preferences(true, vec![Channel::Feishu]))
        .unwrap();
    dispatch(&manager, &page, completion("no-page")).await;
    assert!(receiver.try_recv().is_err());
    assert!(turn_usage(&json!({"used":100,"size":200})).is_none());
}
