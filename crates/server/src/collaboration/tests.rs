use super::*;
use aow_acp::{AcpHost, AcpService, Content};
use aow_db::tasks::TaskResource;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tower::ServiceExt;

struct Host;
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")
        ]}}}).to_string())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        unreachable!()
    }
}

fn state(root: &Path) -> AppState {
    let mut state =
        AppState::with_state_dir(PathBuf::new(), root.into(), root.join("terminald.sock")).unwrap();
    state.acp = AcpService::new(Arc::new(Host), Some(root.join("acp"))).unwrap();
    state
}

fn content(text: &str) -> Vec<Content> {
    serde_json::from_value(json!([{"type":"text","text":text}])).unwrap()
}

async fn task(state: &AppState, root: &Path, id: &str) -> TaskRecord {
    let session = state
        .acp
        .prepare_session("test", root.into())
        .await
        .unwrap();
    let task = TaskRecord {
        id: id.into(),
        project_id: "project".into(),
        status: "todo".into(),
        prompt: id.into(),
        agent_id: "test".into(),
        agent_role_id: None,
        is_router: id == "router",
        source_task_id: None,
        source_thread_id: None,
        workspace_intent: None,
        target_task_id: None,
        target_revision: None,
        workspace: root.to_string_lossy().into_owned(),
        workspace_mode: "existing".into(),
        slug: None,
        initial_thread_id: Some(session.thread_id.clone()),
        pinned: false,
        archived: false,
        dependencies: vec![],
        resources: vec![TaskResource {
            kind: "acp".into(),
            id: session.thread_id,
        }],
    };
    state.aow.task_db().insert(&task).unwrap();
    task
}

async fn until(mut check: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("collaboration should progress without model polling");
}

fn worker(state: &AppState) -> worker::Worker {
    worker::Worker {
        manager: state.aow.clone(),
        acp: state.acp.clone(),
        controls: state.acp_controls.clone(),
        events: state.workspace_events.clone(),
    }
}

async fn call(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn a_busy_source_gets_one_return_after_becoming_idle_even_when_inbox_is_read_and_archived() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let router = task(&state, directory.path(), "router").await;
    let source_thread = router.initial_thread_id.as_deref().unwrap();
    state.acp.resume(source_thread).await.unwrap();
    state
        .acp
        .start_prompt(source_thread, content("wait"))
        .unwrap();
    let source = source(&state, &router, source_thread).unwrap();
    let child = task(&state, directory.path(), "child").await;
    let mut run = new_run(
        &child,
        "run-child".into(),
        "Count Rust code".into(),
        Some(source.clone()),
    );
    run.execution.as_mut().unwrap().workspace_intent = Some("analyze".into());
    let db = state.aow.task_db();
    db.enqueue_run(&run).unwrap();
    state.collaboration.start(&state);
    until(|| db.run(&run.id).unwrap().unwrap().terminal()).await;
    let event = db.inbox(None, false, false, 0, 30).unwrap().items.remove(0);
    assert_eq!(event.run.result.as_deref(), Some("Done"));
    assert_eq!(event.delivery.unwrap().state, "pending");
    assert_eq!(state.acp.snapshot(source_thread).unwrap().status, "working");
    db.update_inbox(&event.id, Some(true), Some(true)).unwrap();
    state.acp.cancel(source_thread).unwrap();
    until(|| {
        db.inbox(None, false, true, 0, 30).unwrap().items[0]
            .delivery
            .as_ref()
            .unwrap()
            .state
            == "processed"
    })
    .await;
    let snapshot = state.acp.snapshot(source_thread).unwrap();
    assert_eq!(snapshot.prompt_receipts.len(), 1);
    let wakeups = db.task_runs("router").unwrap();
    assert_eq!(wakeups.len(), 1);
    assert_eq!(wakeups[0].trigger, "wakeup");
    assert_eq!(
        wakeups[0].caused_by_run_id.as_deref(),
        Some(run.id.as_str())
    );
    assert!(wakeups[0].terminal());
    let wakeup = snapshot
        .entries
        .iter()
        .find(|entry| entry.content["_meta"]["aow/event"]["id"] == event.id)
        .unwrap();
    let text = wakeup.content["text"].as_str().unwrap();
    assert!(text.contains(&run.id));
    assert!(text.contains(source.turn_id.as_ref().unwrap()));
    assert!(text.contains(source.message_id.as_ref().unwrap()));
    assert!(text.contains("Count Rust code"));
    assert_eq!(db.inbox(None, false, false, 0, 30).unwrap().unread, 0);
    state.acp.close_session(source_thread).await.unwrap();
    state.acp.close_session(&run.thread_id).await.unwrap();
}

#[tokio::test]
async fn restart_reconciles_both_crash_windows_without_repeating_prompts() {
    let directory = tempfile::tempdir().unwrap();
    let first = state(directory.path());
    let router = task(&first, directory.path(), "router").await;
    let source_thread = router.initial_thread_id.as_deref().unwrap();
    first.acp.resume(source_thread).await.unwrap();
    let child = task(&first, directory.path(), "child").await;
    let run = new_run(
        &child,
        "durable-run".into(),
        "Inspect".into(),
        Some(source(&first, &router, source_thread).unwrap()),
    );
    first.aow.task_db().enqueue_run(&run).unwrap();
    first.acp.resume(&run.thread_id).await.unwrap();
    // Crash window: ACP has the durable receipt/result, tasks DB still says queued.
    first
        .acp
        .start_prompt_once(&run.thread_id, &run.id, content(&run.prompt))
        .unwrap();
    until(|| {
        first.acp.snapshot(&run.thread_id).unwrap().prompt_receipts[&run.id].status != "in_progress"
    })
    .await;
    first.acp.close_session(&run.thread_id).await.unwrap();
    first.acp.close_session(source_thread).await.unwrap();
    first.acp.flush_threads_on_quit().unwrap();
    drop(first);
    let second = state(directory.path());
    worker(&second)
        .execution(second.aow.task_db().run(&run.id).unwrap().unwrap())
        .await
        .unwrap();
    let delivery = second.aow.task_db().active_deliveries().unwrap().remove(0);
    worker(&second).wakeup(delivery.clone()).await.unwrap();
    until(|| {
        second.acp.snapshot(source_thread).unwrap().prompt_receipts[&delivery.prompt_id].status
            != "in_progress"
    })
    .await;
    // Crash window: wakeup finished, delivery hasn't been acknowledged in tasks DB.
    assert_eq!(
        second.aow.task_db().active_deliveries().unwrap()[0].state,
        "admitted"
    );
    second.acp.close_session(source_thread).await.unwrap();
    second.acp.flush_threads_on_quit().unwrap();
    drop(second);
    let third = state(directory.path());
    third.collaboration.start(&third);
    until(|| third.aow.task_db().active_deliveries().unwrap().is_empty()).await;
    let page = third
        .aow
        .task_db()
        .inbox(None, false, false, 0, 30)
        .unwrap();
    assert_eq!(
        third.aow.task_db().task_runs("router").unwrap().len(),
        1,
        "restart reuses the persisted wakeup run"
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].delivery.as_ref().unwrap().state, "processed");
    let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2, "one child prompt and one return");
}

#[tokio::test]
async fn send_has_per_request_provenance_fixed_results_and_idempotency() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let first = task(&state, directory.path(), "first-source").await;
    let second = task(&state, directory.path(), "second-source").await;
    let target = task(&state, directory.path(), "target").await;
    let app = crate::aow::task_cli_routes()
        .merge(crate::inbox::routes())
        .with_state(state.clone());
    let first_request = json!({"request_id":"first","source_thread_id":first.initial_thread_id,"prompt":"First request"});
    let (status, accepted) =
        call(&app, "POST", "/v1/tasks/target/send", first_request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/send", first_request.clone())
            .await
            .1["run_id"],
        accepted["run_id"]
    );
    let mut changed = first_request;
    changed["prompt"] = json!("Different request");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/send", changed).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, _) = call(&app, "POST", "/v1/tasks/target/send", json!({
        "request_id":"second","source_thread_id":second.initial_thread_id,"prompt":"Second request"
    })).await;
    assert_eq!(status, StatusCode::OK);
    until(|| {
        state
            .aow
            .task_db()
            .task_runs("target")
            .unwrap()
            .iter()
            .all(TaskRun::terminal)
    })
    .await;
    let db = state.aow.task_db();
    let runs = db.task_runs("target").unwrap();
    assert_eq!(runs.len(), 2);
    assert_ne!(
        runs[0].source.as_ref().unwrap().thread_id,
        runs[1].source.as_ref().unwrap().thread_id
    );
    db.remove("target").unwrap();
    let (status, old) = call(
        &app,
        "GET",
        &format!(
            "/v1/tasks/target?run_id={}",
            accepted["run_id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(old["run"]["prompt"], "First request");
    assert_eq!(old["run"]["result"], "Done");
    state
        .acp
        .close_session(target.initial_thread_id.as_deref().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn missing_source_blocks_only_delivery_and_manual_retry_preserves_result() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let parent = task(&state, directory.path(), "parent").await;
    let child = task(&state, directory.path(), "child").await;
    let run = new_run(
        &child,
        "failure".into(),
        "Inspect".into(),
        Some(
            source(
                &state,
                &parent,
                parent.initial_thread_id.as_deref().unwrap(),
            )
            .unwrap(),
        ),
    );
    let db = state.aow.task_db();
    db.enqueue_run(&run).unwrap();
    db.finish_run(
        &run.id,
        "failed",
        None,
        Some("Provider could not start".into()),
        now(),
    )
    .unwrap();
    state
        .acp
        .delete_session(parent.initial_thread_id.as_deref().unwrap())
        .await
        .unwrap();
    worker(&state)
        .wakeup(db.active_deliveries().unwrap().remove(0))
        .await
        .unwrap();
    let event = db.inbox(None, false, false, 0, 30).unwrap().items.remove(0);
    assert_eq!(event.run.state, "failed");
    assert_eq!(event.delivery.unwrap().state, "blocked");
    db.retry_delivery(&event.id, "retry").unwrap();
    worker(&state)
        .wakeup(db.active_deliveries().unwrap().remove(0))
        .await
        .unwrap();
    assert_eq!(
        db.run(&run.id).unwrap().unwrap().error.as_deref(),
        Some("Provider could not start")
    );
    assert!(db.active_deliveries().unwrap().is_empty());
    let attempts = db.task_runs("parent").unwrap();
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts
            .iter()
            .all(|run| run.trigger == "wakeup" && run.terminal())
    );
    assert_ne!(attempts[0].id, attempts[1].id);
}

#[tokio::test]
async fn role_conversations_share_workspace_admission_without_sharing_results() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let developer = task(&state, directory.path(), "developer").await;
    let reviewer = task(&state, directory.path(), "reviewer").await;
    let mut change = new_run(&developer, "implement".into(), "wait".into(), None);
    change.execution.as_mut().unwrap().workspace_intent = Some("change".into());
    let mut review = new_run(&reviewer, "review".into(), "Review".into(), None);
    review.execution.as_mut().unwrap().workspace_intent = Some("review".into());
    let db = state.aow.task_db();
    db.enqueue_run(&change).unwrap();
    db.enqueue_run(&review).unwrap();
    let worker = worker(&state);
    worker.execution(change.clone()).await.unwrap();
    worker.execution(review.clone()).await.unwrap();
    assert_eq!(db.run(&review.id).unwrap().unwrap().state, "queued");
    assert!(
        state
            .acp
            .snapshot(&review.thread_id)
            .unwrap()
            .prompt_receipts
            .is_empty()
    );
    db.set_status("developer", "cancelled").unwrap();
    worker
        .execution(db.run(&change.id).unwrap().unwrap())
        .await
        .unwrap();
    let queued = new_run(
        &developer,
        "queued-before-cancellation".into(),
        "Must not execute".into(),
        None,
    );
    db.enqueue_run(&queued).unwrap();
    worker.execution(queued.clone()).await.unwrap();
    assert_eq!(db.run(&queued.id).unwrap().unwrap().state, "interrupted");
    assert!(
        !state
            .acp
            .snapshot(&queued.thread_id)
            .unwrap()
            .prompt_receipts
            .contains_key(&queued.id)
    );
    until(|| {
        state
            .acp
            .snapshot(&change.thread_id)
            .unwrap()
            .prompt_receipts[&change.id]
            .status
            != "in_progress"
    })
    .await;
    worker.execution(change.clone()).await.unwrap();
    worker.execution(review.clone()).await.unwrap();
    until(|| {
        state
            .acp
            .snapshot(&review.thread_id)
            .unwrap()
            .prompt_receipts[&review.id]
            .status
            != "in_progress"
    })
    .await;
    worker.execution(review.clone()).await.unwrap();
    assert_eq!(db.run(&change.id).unwrap().unwrap().state, "interrupted");
    assert_eq!(
        db.run(&review.id).unwrap().unwrap().result.as_deref(),
        Some("Done")
    );
    state.acp.close_session(&change.thread_id).await.unwrap();
    state.acp.close_session(&review.thread_id).await.unwrap();
}

#[tokio::test]
async fn followups_from_wakeups_keep_user_intent_and_reference_the_new_run() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let router = task(&state, directory.path(), "router").await;
    let child = task(&state, directory.path(), "child").await;
    let thread = router.initial_thread_id.as_deref().unwrap();
    let mut origin = source(&state, &router, thread).unwrap();
    origin.request = "Implement login and review it".into();
    let initial = new_run(
        &child,
        "first".into(),
        "Implement login".into(),
        Some(origin),
    );
    let db = state.aow.task_db();
    db.enqueue_run(&initial).unwrap();
    db.finish_run(
        &initial.id,
        "result_available",
        Some("Implemented".into()),
        None,
        now(),
    )
    .unwrap();
    let delivery = db.active_deliveries().unwrap().remove(0);
    let mut wakeup = new_run(
        &router,
        delivery.prompt_id.clone(),
        "WAKEUP envelope".into(),
        None,
    );
    wakeup.id = delivery.prompt_id.clone();
    db.enqueue_wakeup(&delivery.id, &wakeup).unwrap();
    state.acp.resume(thread).await.unwrap();
    state
        .acp
        .start_prompt_once(thread, &wakeup.id, content("wait"))
        .unwrap();
    let followup = source(&state, &router, thread).unwrap();
    assert_eq!(followup.request, "Implement login and review it");
    assert_eq!(followup.parent_run_id.as_deref(), Some(wakeup.id.as_str()));
    assert_eq!(followup.turn_id.as_deref(), Some(wakeup.id.as_str()));
    state.acp.cancel(thread).unwrap();
    state.acp.close_session(thread).await.unwrap();
}
