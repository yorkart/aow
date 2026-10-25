use super::*;

#[test]
fn retained_ancestry_survives_restart_and_rejects_reparenting() {
    let directory = tempfile::tempdir().unwrap();
    let store = InstanceStore::persistent(directory.path()).unwrap();
    let parent = InstanceRef::terminal("root");
    let child = InstanceRef::acp("child");
    let info = |instance, parent| InstanceInfo {
        instance,
        parent,
        title: "instance".into(),
        cwd: "/repo".into(),
        agent_id: None,
        url: None,
        tab_id: None,
        deleted: false,
    };
    store
        .record_all(vec![
            info(parent.clone(), None),
            info(child.clone(), Some(parent.clone())),
        ])
        .unwrap();
    assert!(store.record_all(vec![info(child.clone(), None)]).is_err());
    let restored = InstanceStore::persistent(directory.path()).unwrap();
    let mut state = AppState::new(Default::default());
    state.instances = restored;
    let graph = graph(&state).unwrap();
    assert_eq!(graph[&child].parent, Some(parent.clone()));
    assert!(graph[&parent].deleted && graph[&child].deleted);
    assert!(preserve_parent(&state, Some(&parent)).is_err());
}

#[test]
fn failed_persistence_does_not_publish_a_creation_record() {
    let directory = tempfile::tempdir().unwrap();
    let store = InstanceStore::persistent(directory.path()).unwrap();
    std::fs::create_dir_all(directory.path().join("state/instances.json")).unwrap();
    assert!(
        store
            .record_all(vec![InstanceInfo {
                instance: InstanceRef::acp("child"),
                parent: None,
                title: "child".into(),
                cwd: "/repo".into(),
                agent_id: None,
                url: None,
                tab_id: None,
                deleted: false
            }])
            .is_err()
    );
    assert!(store.snapshot().unwrap().is_empty());
}

#[tokio::test]
async fn acp_metadata_invalidates_the_graph_without_an_http_action() {
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::with_state_dir(
        Default::default(),
        directory.path().join("state"),
        directory.path().join("missing.sock"),
    )
    .unwrap();
    let settings = state.aow.acp_settings().unwrap();
    let adapter = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../acp/tests/fixtures/lineage-agent.mjs");
    state
        .aow
        .save_acp_settings(
            json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[adapter]}}})
                .to_string(),
            settings.revision,
        )
        .await
        .unwrap();
    let mut events = state.workspace_events.subscribe();
    let session = state
        .acp
        .prepare_session("test", directory.path().into())
        .await
        .unwrap();
    assert!(events.has_changed().unwrap());
    state.acp.resume(&session.thread_id).await.unwrap();
    events.borrow_and_update();
    state
        .acp
        .start_prompt(
            &session.thread_id,
            serde_json::from_value(json!([{"type":"text","text":"rename"}])).unwrap(),
        )
        .unwrap();
    let reference = InstanceRef::acp(&session.thread_id);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            events.changed().await.unwrap();
            let event = serde_json::to_value(events.borrow_and_update().clone()).unwrap();
            assert!(event["instances"].as_u64().unwrap() > 0);
            if graph(&state).unwrap()[&reference].title == "Provider title" {
                break;
            }
        }
    })
    .await
    .unwrap();
    state.acp.delete_session(&session.thread_id).await.unwrap();
    assert!(events.has_changed().unwrap());
    assert!(!graph(&state).unwrap().contains_key(&reference));
}
