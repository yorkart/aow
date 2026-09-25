use super::*;
use aow_automations::{RunEvent, RunSource, Task, runner::initial_run, store::RunWriter};
use std::sync::atomic::{AtomicUsize, Ordering};

fn task() -> Task {
    serde_json::from_value(serde_json::json!({
        "id":"12345678", "revision":1,
        "created_at":"2026-01-01T00:00:00Z", "updated_at":"2026-01-01T00:00:00Z",
        "name":"每日检查", "prompt":"check", "agent":"codex", "project_id":"project",
        "workspace_mode":"existing", "workspace_path":"/tmp", "cron":"0 9 * * *",
        "max_concurrent_runs":3, "enabled":true, "failure_notification":"feishu",
        "project_name":"Project", "repository_path":"/tmp",
        "launch":{"executable":"/bin/false","args":[],"environment":{}}, "scheduler_error":null
    }))
    .unwrap()
}

fn start(store: &Store, task: &Task, id: &str) -> RunWriter {
    store
        .create_run(&initial_run(task, id.into(), RunSource::Scheduled), task)
        .unwrap()
}

fn finish(writer: &mut RunWriter, status: RunStatus) {
    writer
        .append(&RunEvent::Finished {
            at: Utc::now(),
            status,
            exit_code: Some(1),
            message: Some("Agent 退出失败".into()),
            duration_ms: 1234,
        })
        .unwrap();
}

fn state(store: &Store, id: &str) -> DeliveryState {
    serde_json::from_slice(
        &fs::read(store.root.join("runs/12345678").join(id).join(STATE_FILE)).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn wechat_choice_is_taken_from_run_snapshot_even_after_task_switches_to_feishu() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(root.path().into()).unwrap());
    let mut task = task();
    task.input.failure_notification = Some(FailureNotification::Wechat);
    finish(
        &mut start(&store, &task, "wechat-failed"),
        RunStatus::Failed,
    );
    task.input.failure_notification = Some(FailureNotification::Feishu);
    store.save_task(&task).unwrap();
    poll(
        store.clone(),
        |run, channel, _| {
            assert_eq!(run.id, "wechat-failed");
            assert_eq!(channel, FailureNotification::Wechat);
            async { Ok(true) }
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(state(&store, "wechat-failed").status, DeliveryStatus::Sent);
}

#[tokio::test]
async fn server_observer_starts_without_frontend_requests() {
    let root = tempfile::tempdir().unwrap();
    let manager = crate::automations::AutomationManager::new(
        root.path().into(),
        crate::notifications::NotificationManager::in_memory(),
    )
    .unwrap();
    finish(
        &mut start(&manager.store, &task(), "background"),
        RunStatus::Failed,
    );
    let path = manager
        .store
        .root
        .join("runs/12345678/background")
        .join(STATE_FILE);
    tokio::time::timeout(std::time::Duration::from_secs(7), async {
        loop {
            if let Ok(bytes) = fs::read(&path)
                && serde_json::from_slice::<DeliveryState>(&bytes)
                    .unwrap()
                    .status
                    == DeliveryStatus::SkippedMissingBot
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        manager
            .store
            .read_run("12345678", "background")
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Failed
    );
}

#[tokio::test]
async fn observes_snapshot_failures_in_any_completion_order_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(root.path().into()).unwrap());
    let mut task = task();
    let mut older = start(&store, &task, "run-1");
    let mut newer = start(&store, &task, "run-2");
    finish(&mut newer, RunStatus::Failed);
    drop(newer);
    // Current task changes never rewrite the notification policy of a run.
    task.input.failure_notification = None;
    task.input.name = "修改后的任务".into();
    store.save_task(&task).unwrap();
    let sent = std::sync::Mutex::new(Vec::new());
    poll(
        store.clone(),
        |run, channel, id| {
            assert_eq!(channel, FailureNotification::Feishu);
            assert_eq!(run.task_name, "每日检查");
            assert!(Uuid::parse_str(&id).is_ok());
            sent.lock().unwrap().push(run.id);
            async { Ok(true) }
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(*sent.lock().unwrap(), ["run-2"]);
    assert!(
        !store
            .root
            .join("runs/12345678/run-1")
            .join(STATE_FILE)
            .exists()
    );
    finish(&mut older, RunStatus::Failed);
    drop(older);
    let restarted = Arc::new(Store::open(root.path().into()).unwrap());
    poll(
        restarted.clone(),
        |run, _, _| {
            sent.lock().unwrap().push(run.id);
            async { Ok(true) }
        },
        false,
    )
    .await
    .unwrap();
    poll(
        restarted,
        |_, _, _| async { panic!("duplicate notification") },
        false,
    )
    .await
    .unwrap();
    assert_eq!(*sent.lock().unwrap(), ["run-2", "run-1"]);
    assert_eq!(state(&store, "run-1").status, DeliveryStatus::Sent);
}

#[tokio::test]
async fn only_failed_opted_in_runs_notify_and_missing_bot_does_not_replay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(root.path().into()).unwrap());
    let mut task = task();
    for (id, status) in [
        ("complete", RunStatus::Completed),
        ("skip", RunStatus::Skipped),
        ("interrupt", RunStatus::Interrupted),
        ("failed", RunStatus::Failed),
    ] {
        finish(&mut start(&store, &task, id), status);
    }
    // A runner that disappears before its finished event is interrupted, not failed.
    drop(start(&store, &task, "crashed"));
    task.input.failure_notification = None;
    finish(&mut start(&store, &task, "disabled"), RunStatus::Failed);
    let calls = AtomicUsize::new(0);
    poll(
        store.clone(),
        |run, _, _| {
            assert_eq!(run.id, "failed");
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(false) }
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        state(&store, "failed").status,
        DeliveryStatus::SkippedMissingBot
    );
    assert!(state(&store, "failed").message.unwrap().contains("未配置"));
    for id in ["complete", "skip", "interrupt", "crashed", "disabled"] {
        assert_eq!(state(&store, id).status, DeliveryStatus::NotRequested);
    }
    // Configuring a bot later affects new runs, not previously skipped reminders.
    task.input.failure_notification = Some(FailureNotification::Feishu);
    finish(&mut start(&store, &task, "later"), RunStatus::Failed);
    poll(
        store.clone(),
        |run, _, _| {
            assert_eq!(run.id, "later");
            async { Ok(true) }
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(state(&store, "later").status, DeliveryStatus::Sent);
}

#[tokio::test]
async fn overlapping_observers_and_uncertain_deliveries_never_replay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(root.path().into()).unwrap());
    finish(&mut start(&store, &task(), "failed"), RunStatus::Failed);
    let held = scan(&store).unwrap().unwrap();
    // A concurrent fork may keep the same open file description alive until exec.
    let inherited_lock = held._lock.0.try_clone().unwrap();
    poll(
        store.clone(),
        |_, _, _| async { panic!("another observer owns the lock") },
        false,
    )
    .await
    .unwrap();
    drop(held);
    poll(
        store.clone(),
        |_, _, _| async { anyhow::bail!("network failed") },
        false,
    )
    .await
    .unwrap();
    assert_eq!(state(&store, "failed").status, DeliveryStatus::Failed);
    drop(inherited_lock);
    // Simulate shutdown between HTTP delivery and persisting its outcome.
    finish(&mut start(&store, &task(), "uncertain"), RunStatus::Failed);
    save(
        &store.root.join("runs/12345678/uncertain").join(STATE_FILE),
        DeliveryStatus::Sending,
        Some("delivery".into()),
        None,
    )
    .unwrap();
    poll(
        store.clone(),
        |_, _, _| async { panic!("must not retry ambiguous delivery") },
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        state(&store, "uncertain").status,
        DeliveryStatus::Unconfirmed
    );
    assert_eq!(
        store
            .read_run("12345678", "failed")
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Failed
    );
}

#[tokio::test]
async fn processes_offline_failures_before_pruning_history() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(root.path().into()).unwrap());
    let mut task = task();
    let late = std::sync::Mutex::new(Some(start(&store, &task, "0000-late")));
    finish(&mut start(&store, &task, "0000"), RunStatus::Failed);
    task.input.failure_notification = None;
    for id in 1..=super::super::RUN_HISTORY_RETENTION {
        finish(
            &mut start(&store, &task, &format!("{id:04}")),
            RunStatus::Completed,
        );
    }
    let calls = AtomicUsize::new(0);
    poll(
        store.clone(),
        |run, _, _| {
            assert_eq!(run.id, "0000");
            calls.fetch_add(1, Ordering::SeqCst);
            // Finish an older active run after the scan but before cleanup.
            finish(&mut late.lock().unwrap().take().unwrap(), RunStatus::Failed);
            async { Ok(true) }
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!store.root.join("runs/12345678/0000").exists());
    assert!(store.root.join("runs/12345678/0000-late").exists());
    poll(
        store.clone(),
        |run, _, _| {
            assert_eq!(run.id, "0000-late");
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(true) }
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!store.root.join("runs/12345678/0000-late").exists());
}

#[test]
fn legacy_tasks_default_to_no_reminder_and_synced_preferences_validate_without_bot() {
    let mut value = serde_json::to_value(task()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("failure_notification");
    let legacy: Task = serde_json::from_value(value).unwrap();
    assert_eq!(legacy.input.failure_notification, None);
    task().input.validate().unwrap();
}
