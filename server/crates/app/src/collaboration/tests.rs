use super::*;
use aow_db::tasks::{EventKind, EventTarget, TaskEvent, TaskResource, TaskStatus};
use aow_host_client::acp::AcpClient;
use aow_host_client::{acp::Content, testing::AcpFixture};
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
impl AcpFixture for Host {
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/acp/lineage-agent.mjs")
        ]}}}).to_string())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
}

fn state(root: &Path) -> AppState {
    let mut state =
        AppState::for_test_with_state_dir(PathBuf::new(), root.into(), root.join("terminald.sock"))
            .unwrap();
    state.acp = AcpClient::for_test_host(Arc::new(Host), Some(root.join("acp"))).unwrap();
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
        status: TaskStatus::Todo,
        created_at: Some(1_700_000_000_000),
        updated_at: Some(1_700_000_000_000),
        completed_at: None,
        prompt: id.into(),
        agent_id: "test".into(),
        agent_role_id: None,
        is_router: id == "router",
        source_task_id: None,
        source_thread_id: None,
        workspace_intent: Some("analyze".into()),
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
    state.tasks.test_db().insert(&task).unwrap();
    task
}

async fn until<F: std::future::Future<Output = bool>>(mut check: impl FnMut() -> F) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !check().await {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("collaboration should progress without model polling");
}

fn worker(state: &AppState) -> worker::Worker {
    worker::Worker {
        tasks: state.tasks.clone(),
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
async fn multiple_turns_do_not_report_but_explicit_reports_wake_the_busy_parent_once() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let parent = task(&state, directory.path(), "parent").await;
    let child = task(&state, directory.path(), "child").await;
    let parent_thread = parent.initial_thread_id.as_deref().unwrap();
    let child_thread = child.initial_thread_id.as_deref().unwrap();
    let db = state.tasks.test_db();
    state.acp.resume(parent_thread).await.unwrap();
    state
        .acp
        .start_prompt(parent_thread, content("wait"))
        .await
        .unwrap();
    for id in ["first", "correction"] {
        let message = new_message(
            &child,
            id.into(),
            "Check again".into(),
            Some(source(&state, &parent, parent_thread).await.unwrap()),
        );
        db.enqueue_message(&message).unwrap();
        worker(&state).deliver(message.clone()).await.unwrap();
        until(|| async {
            state
                .acp
                .snapshot(child_thread)
                .await
                .unwrap()
                .prompt_receipts
                .get(&message.id)
                .is_some_and(|r| r.status != "in_progress")
        })
        .await;
    }
    assert!(
        db.inbox(None, false, false, 0, 30)
            .unwrap()
            .items
            .is_empty()
    );
    db.set_status("child", TaskStatus::Blocked).unwrap();
    let app = crate::tasks::cli_routes().with_state(state.clone());
    let request = json!({"request_id":"poc-conclusion","source_thread_id":child_thread,"kind":"progress","message":"POC confirmed","target_thread_id":parent_thread});
    let (status, event) = call(&app, "POST", "/v1/tasks/child/report", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{event}");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/child/report", request.clone())
            .await
            .1,
        event
    );
    assert_eq!(event["source"]["thread_id"], parent_thread);
    assert_eq!(event["source"]["request"], "wait");
    let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
    let prompts: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let child_session = state.acp.snapshot(child_thread).await.unwrap();
    for sent in prompts
        .iter()
        .filter(|sent| sent["sessionId"] == json!(child_session.session_id))
    {
        assert_eq!(
            sent["prompt"],
            json!([{ "type":"text", "text":"Check again" }]),
            "source correlation stays in the database, not the provider prompt"
        );
    }
    let event_id = event["id"].as_str().unwrap();
    assert_eq!(db.status("child").unwrap(), Some(TaskStatus::Blocked));
    db.update_inbox(event_id, Some(true), Some(true)).unwrap();
    assert_eq!(
        state.acp.snapshot(parent_thread).await.unwrap().status,
        "working"
    );
    state.acp.cancel(parent_thread).await.unwrap();
    until(|| async {
        db.inbox(None, false, true, 0, 30).unwrap().items[0]
            .delivery
            .as_ref()
            .unwrap()
            .state
            == DeliveryState::Delivered
    })
    .await;
    until(|| async { state.acp.snapshot(parent_thread).await.unwrap().status == "idle" }).await;
    let snapshot = state.acp.snapshot(parent_thread).await.unwrap();
    assert_eq!(snapshot.prompt_receipts.len(), 1);
    let wakeup_log =
        std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
    let wakeup: Value = serde_json::from_str(wakeup_log.lines().last().unwrap()).unwrap();
    assert_eq!(wakeup["prompt"].as_array().unwrap().len(), 1);
    assert!(wakeup["prompt"][0]["_meta"]["aow/instructions"].is_null());
    assert!(
        snapshot
            .entries
            .iter()
            .any(|entry| entry.content["_meta"]["aow/event"]["id"] == event_id)
    );
    assert_eq!(
        db.inbox(None, false, true, 0, 30).unwrap().items.len(),
        1,
        "Consuming an event must not publish another report"
    );
    let mut conflicting = request;
    conflicting["message"] = json!("Different conclusion");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/child/report", conflicting)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(call(&app,"POST","/v1/tasks/parent/report",json!({"request_id":"wrong-owner","source_thread_id":child_thread,"kind":"result","message":"Not mine"})).await.0,StatusCode::BAD_REQUEST);
    state
        .acp
        .start_prompt(child_thread, content("New direct instruction"))
        .await
        .unwrap();
    until(|| async { state.acp.snapshot(child_thread).await.unwrap().status == "idle" }).await;
    let direct = report_source(&state, &child, child_thread).await.unwrap();
    assert_eq!(direct.thread_id, child_thread);
    assert_eq!(
        direct.request, "New direct instruction",
        "a later user turn must not reuse an earlier assignment's source"
    );
    state.acp.close_session(child_thread).await.unwrap();
    state.acp.close_session(parent_thread).await.unwrap();
}

#[tokio::test]
async fn restart_after_acp_admission_does_not_repeat_a_message_or_create_an_inbox_event() {
    let directory = tempfile::tempdir().unwrap();
    let first = state(directory.path());
    let target = task(&first, directory.path(), "target").await;
    let message = new_message(&target, "request".into(), "Inspect".into(), None);
    first.tasks.test_db().enqueue_message(&message).unwrap();
    first.acp.resume(&message.thread_id).await.unwrap();
    first
        .acp
        .start_prompt_once(&message.thread_id, &message.id, content("Inspect"))
        .await
        .unwrap();
    until(|| async {
        first
            .acp
            .snapshot(&message.thread_id)
            .await
            .unwrap()
            .prompt_receipts[&message.id]
            .status
            != "in_progress"
    })
    .await;
    first.acp.close_session(&message.thread_id).await.unwrap();
    drop(first);
    let second = state(directory.path());
    worker(&second).deliver(message.clone()).await.unwrap();
    assert_eq!(
        second
            .tasks
            .test_db()
            .message_for_request("target", "request")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Delivered
    );
    assert_eq!(
        second
            .acp
            .snapshot(&message.thread_id)
            .await
            .unwrap()
            .prompt_receipts
            .len(),
        1
    );
    assert!(
        second
            .tasks
            .test_db()
            .inbox(None, false, false, 0, 30)
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn send_queues_without_waiting_and_enforces_identity_and_task_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let parent = task(&state, directory.path(), "parent").await;
    let target = task(&state, directory.path(), "target").await;
    let source = parent.initial_thread_id.as_deref().unwrap();
    let thread = target.initial_thread_id.as_deref().unwrap();
    state.acp.resume(thread).await.unwrap();
    state
        .acp
        .start_prompt(thread, content("wait"))
        .await
        .unwrap();
    let app = crate::tasks::cli_routes().with_state(state.clone());
    let request = json!({"request_id":"send-1","source_thread_id":source,"prompt":"Continue"});
    let (status, accepted) = call(&app, "POST", "/v1/tasks/target/send", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["delivery"], "pending");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/send", request.clone())
            .await
            .1,
        accepted
    );
    let mut conflict = request.clone();
    conflict["prompt"] = json!("Other work");
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/send", conflict)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut wrong = request.clone();
    wrong["thread_id"] = json!(source);
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/send", wrong).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "POST", "/v1/tasks/target/runs", request).await.0,
        StatusCode::NOT_FOUND
    );
    state.acp.cancel(thread).await.unwrap();
    let db = state.tasks.test_db();
    until(|| async {
        db.message_for_request("target", "send-send-1")
            .unwrap()
            .unwrap()
            .state
            == DeliveryState::Delivered
    })
    .await;
    until(|| async { state.acp.snapshot(thread).await.unwrap().status == "idle" }).await;
    assert!(
        db.inbox(None, false, false, 0, 30)
            .unwrap()
            .items
            .is_empty()
    );
    state.acp.close_session(thread).await.unwrap();
}

#[tokio::test]
async fn blocked_delivery_can_retry_without_losing_report_or_replaying_accepted_messages() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let parent = task(&state, directory.path(), "parent").await;
    let child = task(&state, directory.path(), "child").await;
    let db = state.tasks.test_db();
    let event = TaskEvent {
        id: "report".into(),
        request_id: "report".into(),
        project_id: "project".into(),
        task_id: child.id.clone(),
        thread_id: child.initial_thread_id.clone().unwrap(),
        kind: EventKind::Result,
        message: "Confirmed finding".into(),
        target: Some(EventTarget {
            task_id: parent.id.clone(),
            thread_id: parent.initial_thread_id.clone().unwrap(),
        }),
        source: None,
        created_at: now(),
    };
    db.publish_event(&event).unwrap();
    db.update(&parent.id, None, None, Some(true), None).unwrap();
    let mut message = db.pending_messages().unwrap().remove(0);
    message.attempts = 2;
    worker(&state).deliver(message.clone()).await.unwrap();
    assert_eq!(
        db.inbox(None, false, false, 0, 30).unwrap().items[0]
            .delivery
            .as_ref()
            .unwrap()
            .state,
        DeliveryState::Blocked
    );
    db.update(&parent.id, None, None, Some(false), None)
        .unwrap();
    db.retry_delivery(&event.id).unwrap();
    worker(&state)
        .deliver(db.pending_messages().unwrap().remove(0))
        .await
        .unwrap();
    until(|| async { state.acp.snapshot(&message.thread_id).await.unwrap().status == "idle" })
        .await;
    worker(&state).deliver(message.clone()).await.unwrap();
    assert_eq!(
        state
            .acp
            .snapshot(&message.thread_id)
            .await
            .unwrap()
            .prompt_receipts
            .len(),
        1
    );
    assert_eq!(db.event(&event.id).unwrap().unwrap(), event);
    state.acp.close_session(&message.thread_id).await.unwrap();
}

#[tokio::test]
async fn busy_conversations_do_not_starve_other_targets_and_messages_keep_fifo_order() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let db = state.tasks.test_db();
    let mut busy = Vec::new();
    for index in 0..9 {
        let task = task(&state, directory.path(), &format!("busy-{index}")).await;
        let thread = task.initial_thread_id.as_ref().unwrap();
        state.acp.resume(thread).await.unwrap();
        state
            .acp
            .start_prompt(thread, content("wait"))
            .await
            .unwrap();
        db.enqueue_message(&new_message(&task, "queued".into(), "Later".into(), None))
            .unwrap();
        busy.push(thread.clone());
    }
    let target = task(&state, directory.path(), "available").await;
    let thread = target.initial_thread_id.as_ref().unwrap();
    let first = new_message(&target, "first".into(), "wait".into(), None);
    let second = new_message(&target, "second".into(), "Continue".into(), None);
    db.enqueue_message(&first).unwrap();
    db.enqueue_message(&second).unwrap();
    state.collaboration.start(&state);
    until(|| async {
        db.message_for_request(&target.id, "first")
            .unwrap()
            .unwrap()
            .state
            == DeliveryState::Delivered
    })
    .await;
    assert_eq!(
        db.message_for_request(&target.id, "second")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Pending
    );
    for thread in &busy {
        assert_eq!(state.acp.snapshot(thread).await.unwrap().status, "working");
    }
    state.acp.cancel(thread).await.unwrap();
    until(|| async {
        db.message_for_request(&target.id, "second")
            .unwrap()
            .unwrap()
            .state
            == DeliveryState::Delivered
    })
    .await;
    until(|| async { state.acp.snapshot(thread).await.unwrap().status == "idle" }).await;
    assert_eq!(
        state
            .acp
            .snapshot(thread)
            .await
            .unwrap()
            .prompt_receipts
            .len(),
        2
    );
    for thread in busy {
        state.acp.close_session(&thread).await.unwrap();
    }
    state.acp.close_session(thread).await.unwrap();
}

#[tokio::test]
async fn conversation_policy_and_task_cancellation_stop_at_task_ownership_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let parent = task(&state, directory.path(), "parent").await;
    let parent_thread = parent.initial_thread_id.as_ref().unwrap();
    let connection =
        rusqlite::Connection::open(directory.path().join("state/tasks.sqlite3")).unwrap();
    connection.execute("INSERT INTO task_conversation_policies VALUES (?1,?2,?3)",rusqlite::params![parent_thread,parent.id,json!({"workspace":parent.workspace,"workspace_intent":"review","target_task_id":"target","target_revision":"frozen"}).to_string()]).unwrap();
    let mut threads = Vec::new();
    for _ in 0..2 {
        let session = state
            .acp
            .prepare_session_with_options(
                "test",
                directory.path().into(),
                aow_host_client::acp::SessionCreation {
                    parent: Some(aow_host_contract::instance::InstanceRef::acp(parent_thread)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        threads.push(session.thread_id);
    }
    let child = TaskRecord {
        id: "child".into(),
        initial_thread_id: Some(threads[1].clone()),
        resources: vec![TaskResource {
            kind: "acp".into(),
            id: threads[1].clone(),
        }],
        ..parent.clone()
    };
    state.tasks.test_db().insert(&child).unwrap();
    let helper_policy = state
        .tasks
        .task_policy_for_thread(&state.acp, &threads[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(helper_policy.target_revision.as_deref(), Some("frozen"));
    let child_policy = state
        .tasks
        .task_policy_for_thread(&state.acp, &threads[1])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child_policy.workspace_intent.as_deref(), Some("analyze"));
    assert_eq!(child_policy.target_revision, None);
    for thread in &threads {
        state.acp.resume(thread).await.unwrap();
        state
            .acp
            .start_prompt(thread, content("wait"))
            .await
            .unwrap();
    }
    let app = crate::tasks::cli_routes().with_state(state.clone());
    assert_eq!(
        call(&app, "GET", "/v1/tasks/parent", json!({})).await.1["conversations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/tasks/parent/status",
            json!({"status":"cancelled"})
        )
        .await
        .0,
        StatusCode::OK
    );
    until(|| async { state.acp.snapshot(&threads[0]).await.unwrap().status == "idle" }).await;
    assert_eq!(
        state.acp.snapshot(&threads[1]).await.unwrap().status,
        "working"
    );
    for thread in threads {
        state.acp.close_session(&thread).await.unwrap();
    }
}

#[tokio::test]
async fn shared_workspace_review_waits_for_the_active_writer() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let writer = task(&state, directory.path(), "writer").await;
    let reviewer = task(&state, directory.path(), "reviewer").await;
    let writer_thread = writer.initial_thread_id.as_ref().unwrap();
    let reviewer_thread = reviewer.initial_thread_id.as_ref().unwrap();
    let connection =
        rusqlite::Connection::open(directory.path().join("state/tasks.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE tasks SET workspace_intent='review' WHERE id='reviewer'",
            [],
        )
        .unwrap();
    state.acp.resume(writer_thread).await.unwrap();
    state
        .acp
        .start_prompt(writer_thread, content("wait"))
        .await
        .unwrap();
    let message = new_message(
        &reviewer,
        "review".into(),
        "Inspect committed code".into(),
        None,
    );
    let db = state.tasks.test_db();
    db.enqueue_message(&message).unwrap();
    worker(&state).deliver(message.clone()).await.unwrap();
    assert_eq!(
        db.message_for_request("reviewer", "review")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Pending
    );
    assert!(
        state
            .acp
            .snapshot(reviewer_thread)
            .await
            .unwrap()
            .prompt_receipts
            .is_empty()
    );
    state.acp.cancel(writer_thread).await.unwrap();
    until(|| async { state.acp.snapshot(writer_thread).await.unwrap().status == "idle" }).await;
    worker(&state).deliver(message).await.unwrap();
    assert_eq!(
        db.message_for_request("reviewer", "review")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Delivered
    );
    until(|| async { state.acp.snapshot(reviewer_thread).await.unwrap().status == "idle" }).await;
    state.acp.close_session(writer_thread).await.unwrap();
    state.acp.close_session(reviewer_thread).await.unwrap();
}

#[tokio::test]
async fn daemon_outages_do_not_exhaust_durable_message_retries() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let task = task(&state, directory.path(), "outage").await;
    let mut message = new_message(&task, "outage-message".into(), "hello".into(), None);
    message.attempts = 9;
    state.tasks.test_db().enqueue_message(&message).unwrap();
    let mut disconnected = worker(&state);
    disconnected.acp = disconnected.acp.for_test_offline();
    disconnected.deliver(message.clone()).await.unwrap();
    assert_eq!(
        state
            .tasks
            .test_db()
            .message_for_request(&task.id, "outage-message")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Pending
    );
    worker(&state).deliver(message.clone()).await.unwrap();
    assert_eq!(
        state
            .tasks
            .test_db()
            .message_for_request(&task.id, "outage-message")
            .unwrap()
            .unwrap()
            .state,
        DeliveryState::Delivered
    );
}

#[tokio::test]
async fn router_receives_routing_guidance_for_queued_messages_and_wakeups() {
    let directory = tempfile::tempdir().unwrap();
    let state = state(directory.path());
    let router = task(&state, directory.path(), "router").await;
    let child = task(&state, directory.path(), "child").await;
    let thread = router.initial_thread_id.as_ref().unwrap();
    let message = new_message(
        &router,
        "dispatch".into(),
        "Clarify this request".into(),
        Some(MessageSource {
            task_id: child.id.clone(),
            thread_id: child.initial_thread_id.clone().unwrap(),
            turn_id: None,
            message_id: None,
            request: "[@Deleted](mention://role/missing-role) follow up".into(),
        }),
    );
    let db = state.tasks.test_db();
    db.enqueue_message(&message).unwrap();
    worker(&state).deliver(message).await.unwrap();
    until(|| async { state.acp.snapshot(thread).await.unwrap().status == "idle" }).await;
    let event = TaskEvent {
        id: "router-report".into(),
        request_id: "router-report".into(),
        project_id: router.project_id.clone(),
        task_id: child.id.clone(),
        thread_id: child.initial_thread_id.clone().unwrap(),
        kind: EventKind::Result,
        message: "Investigation complete; original requester mentioned [@Deleted](mention://role/missing-role)".into(),
        target: Some(EventTarget {
            task_id: router.id.clone(),
            thread_id: thread.clone(),
        }),
        source: None,
        created_at: now(),
    };
    db.publish_event(&event).unwrap();
    worker(&state)
        .deliver(db.pending_messages().unwrap().pop().unwrap())
        .await
        .unwrap();
    until(|| async { state.acp.snapshot(thread).await.unwrap().status == "idle" }).await;
    let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
    let prompts: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(prompts.len(), 2);
    for sent in &prompts {
        assert!(
            sent["prompt"][0]["text"]
                .as_str()
                .unwrap()
                .contains("AoW Router instructions")
        );
    }
    assert_eq!(prompts[0]["prompt"].as_array().unwrap().len(), 3);
    assert_eq!(
        prompts[0]["prompt"][1]["_meta"]["aow/message"]["source"]["task_id"],
        child.id
    );
    assert_eq!(prompts[0]["prompt"][2]["text"], "Clarify this request");
    assert_eq!(prompts[1]["prompt"].as_array().unwrap().len(), 2);
    assert_eq!(
        prompts[1]["prompt"][1]["_meta"]["aow/event"]["id"],
        event.id
    );
    assert_eq!(db.inbox(None, false, false, 0, 30).unwrap().items.len(), 1);
    state.acp.close_session(thread).await.unwrap();
}
