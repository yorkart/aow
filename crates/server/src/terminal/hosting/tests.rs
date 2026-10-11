use super::*;
use aow_protocol::TerminalAgentProcess;

fn hosting(id: &str) -> TerminalHosting {
    TerminalHosting {
        id: id.into(),
        task_id: "review".into(),
        task_revision: 1,
        task_name: "Review".into(),
        workspace_root: "/tmp".into(),
        agent: "codex".into(),
        session_id: "session".into(),
        process: TerminalAgentProcess {
            pid: 1,
            start_time: "start".into(),
            pi_binding: None,
            cwd: "/tmp".into(),
        },
        phase: TerminalHostingPhase::Waiting,
        phase_started_at: timestamp(),
        max_inputs: 3,
        input_count: 0,
        source_turn_id: None,
        run_id: None,
        error: None,
    }
}

#[test]
fn completion_subscription_only_delivers_future_events_for_the_waiting_instance() {
    use super::super::notifications::TaskStopNotification;
    let manager =
        TerminalManager::in_memory(TerminaldClient::new("/tmp/unused-hosting.sock".into()));
    let event = TaskStopNotification {
        agent: "codex".into(),
        session_id: "session".into(),
        title: "Review".into(),
        cwd: "/tmp".into(),
        turn_id: Some("turn-one".into()),
        conclusion: Some("Done".into()),
        usage: None,
        instance_ids: vec!["pane".into()],
        sources: vec![],
    };
    let _ = manager.inner.task_completions.send(event.clone());
    let mut receiver = manager.subscribe_task_completions();
    assert!(receiver.try_recv().is_err());
    manager.inner.task_completions.send(event.clone()).unwrap();
    let delivered = receiver.try_recv().unwrap();
    let mut current = hosting("active");
    assert!(worker::accepts(&current, "pane", &delivered));
    assert!(!worker::accepts(&current, "other-pane", &delivered));
    current.session_id = "other-session".into();
    assert!(!worker::accepts(&current, "pane", &delivered));
    current.session_id = "session".into();
    current.agent = "claude".into();
    assert!(!worker::accepts(&current, "pane", &delivered));
    current.agent = "codex".into();
    current.phase = TerminalHostingPhase::Reviewing;
    assert!(!worker::accepts(&current, "pane", &delivered));
    current.phase = TerminalHostingPhase::Waiting;
    current.source_turn_id = event.turn_id;
    assert!(!worker::accepts(&current, "pane", &delivered));
    // Reconnecting after a restart also starts at the next event.
    assert!(manager.subscribe_task_completions().try_recv().is_err());
}

#[test]
fn legacy_hosting_metadata_gets_a_finite_default_limit() {
    let mut value = serde_json::to_value(hosting("old")).unwrap();
    value.as_object_mut().unwrap().remove("max_inputs");
    value.as_object_mut().unwrap().remove("input_count");
    let restored: TerminalHosting = serde_json::from_value(value).unwrap();
    assert_eq!(restored.max_inputs, 3);
    assert_eq!(restored.input_count, 0);
}

#[test]
fn takeover_invalidates_late_results_and_old_failures() {
    let manager =
        TerminalManager::in_memory(TerminaldClient::new("/tmp/unused-hosting.sock".into()));
    let pane = super::super::tests::pane("pane", TerminalPaneStatus::Running);
    let tab = super::super::tests::tab_with(
        TerminalLayout::Pane {
            pane_id: "pane".into(),
        },
        vec![pane],
    );
    manager.inner.state.lock().unwrap().tabs.push(tab);
    manager
        .set_hosting("pane", None, Some(hosting("old")))
        .unwrap();
    manager.set_hosting("pane", None, None).unwrap();
    assert!(
        !manager
            .set_hosting("pane", Some("old"), Some(hosting("old")))
            .unwrap()
    );
    manager
        .set_hosting("pane", None, Some(hosting("new")))
        .unwrap();
    assert!(!manager.set_hosting("pane", Some("old"), None).unwrap());
    assert_eq!(manager.hosting("pane").unwrap().unwrap().id, "new");
}

#[tokio::test]
async fn takeover_waits_for_submission_boundary() {
    let manager =
        TerminalManager::in_memory(TerminaldClient::new("/tmp/unused-hosting.sock".into()));
    let first = manager.hosting_gate("pane").unwrap();
    let second = manager.hosting_gate("pane").unwrap();
    let submission = first.lock().await;
    assert!(second.try_lock().is_err());
    drop(submission);
    assert!(second.try_lock().is_ok());
}
