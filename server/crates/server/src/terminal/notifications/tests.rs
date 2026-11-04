use super::*;
#[tokio::test]
async fn source_labels_identify_each_owning_tab_once_and_follow_renames() {
    use serde_json::json;
    let mut tabs = Vec::new();
    for (tab_id, workspace, pane_ids) in [
        ("first", "/workspace/one", vec!["pane-1", "pane-2"]),
        ("second", "/workspace/two", vec!["pane-3"]),
        ("unrelated", "/workspace/three", vec!["pane-4"]),
    ] {
        let panes: Vec<_> = pane_ids
            .iter()
            .map(|id| {
                json!({
                    "id": id, "name": "Obsolete custom pane name", "name_is_custom": true, "cwd": "/somewhere/else",
                    "shell": "/bin/sh", "status": "running", "rows": 24, "cols": 80,
                    "created_at": "now", "updated_at": "now"
                })
            })
            .collect();
        let tab: TerminalTab = serde_json::from_value(json!({
            "id": tab_id, "name": tab_id, "name_is_custom": true,
            "workspace_root": workspace, "panes": panes,
            "layout": {"type": "pane", "pane_id": pane_ids[0]},
            "revision": 1, "created_at": "now", "updated_at": "now"
        }))
        .unwrap();
        tabs.push(tab);
    }

    // Rename after registration; this must not touch the fixed session set.
    tabs[0].name = "排查性能".into();
    tabs[1].name_is_custom = Some(false);
    let detected = serde_json::from_value(json!({
        "agents": {"pane-3": "codex"}, "titles": {"pane-3": "Current visible title"}
    }))
    .unwrap();
    let mut events = vec![TaskStopNotification {
        agent_display_name: None,
        agent: "codex".into(),
        session_id: "shared".into(),
        title: "Node title".into(),
        cwd: "/remote-only".into(),
        turn_id: None,
        conclusion: Some("complete".into()),
        usage: None,
        instance_ids: vec!["pane-1".into(), "pane-2".into(), "pane-3".into()],
        sources: vec![],
    }];
    add_sources(
        &mut events,
        &tabs,
        Some(&detected),
        &crate::aow::AowManager::in_memory(),
    )
    .await;
    assert_eq!(events.len(), 1);
    let sources = &events[0].sources;
    assert_eq!(sources.len(), 2); // two panes in the first tab, one tab label
    assert_eq!(sources[0].project_name, "one");
    assert_eq!(sources[0].workspace_root, "/workspace/one");
    assert_eq!(sources[0].tab_id, "first");
    assert_eq!(sources[0].tab_name, "排查性能");
    assert_eq!(sources[1].project_name, "two");
    assert_eq!(sources[1].tab_id, "second");
    assert_eq!(sources[1].tab_name, "Current visible title");
}

#[tokio::test]
async fn event_endpoint_requires_aow_login() {
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    crate::auth::write_credentials(root.path(), "admin", "test-password");
    let mut state = AppState::for_test(root.path().join("frontend"));
    state.auth = crate::auth::AuthService::persistent(root.path());
    let response = crate::build_router(state)
        .oneshot(
            axum::http::Request::get("/api/terminals/task-stops")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn event_endpoint_streams_only_new_callbacks() {
    use axum::body::HttpBody;
    use tower::ServiceExt;
    let state = AppState::for_test(PathBuf::from("/tmp/frontend-unused"));
    let sender = state.terminals.inner.task_stops.clone();
    let notification = TaskStopNotification {
        agent_display_name: None,
        agent: "codex".into(),
        session_id: "session".into(),
        title: "Title".into(),
        cwd: "/workspace".into(),
        turn_id: None,
        conclusion: Some("本轮结论".into()),
        usage: None,
        instance_ids: vec!["pane".into()],
        sources: Vec::new(),
    };
    let _ = sender.send(notification.clone()); // no replay on browser reconnect
    let response = crate::build_router(state)
        .oneshot(
            axum::http::Request::get("/api/terminals/task-stops")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    sender.send(notification).unwrap();
    let mut body = response.into_body();
    let frame = tokio::time::timeout(
        Duration::from_secs(2),
        std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let data = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(data.contains("event: task-stopped"));
    assert!(data.contains("\"session_id\":\"session\""));
}
