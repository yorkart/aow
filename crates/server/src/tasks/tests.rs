use super::{store::*, *};
use aow_protocol::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

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
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| json!({"message": String::from_utf8_lossy(&body)})),
    )
}
fn task() -> BoardTask {
    BoardTask {
        id: "task-1".into(),
        revision: 1,
        inbox_id: "idea".into(),
        title: "Design a logo".into(),
        description: "No Git needed for a state change".into(),
        status_id: "sketch".into(),
        agent: "custom-codex".into(),
        project_id: "project".into(),
        cwd: "/work".into(),
        tab_id: None,
        pane_id: None,
        execution: TaskExecutionPhase::Ready,
        error: None,
        archived: false,
        created_at: now(),
        updated_at: now(),
        history: vec![],
    }
}

#[test]
fn task_context_escapes_metadata_and_preserves_user_markdown_after_the_context() {
    let mut task = task();
    task.title = "# Review <widget> & API".into();
    task.description =
        "Keep this Markdown unchanged.\n\n```xml\n<aow_task_context />\n```\n".into();
    let prompt = context::prompt(
        &task,
        &[TaskStatus {
            id: "awaiting-acceptance".into(),
            name: "Review </available_statuses> & \"approve\" 'later'".into(),
            color: "#123456".into(),
        }],
    );
    let (context, user) = prompt.split_once("</aow_task_context>\n\n").unwrap();
    assert!(context.starts_with("<aow_task_context>\n"));
    assert!(context.contains("<task_id>task-1</task_id>"));
    assert!(context.contains("<status id=\"awaiting-acceptance\">Review &lt;/available_statuses&gt; &amp; &quot;approve&quot; &apos;later&apos;</status>"));
    assert!(!context.contains("#123456"));
    assert!(!context.contains("--state-dir"));
    assert!(!context.contains("<status id=\"todo\">"));
    assert_eq!(user, format!("{}\n\n{}", task.title, task.description));
}

#[tokio::test]
async fn project_scopes_inbox_and_tasks_and_rejects_cross_project_writes() {
    let state = crate::AppState::with_terminald_socket("".into(), "/missing/socket".into());
    let app = routes().with_state(state.clone());
    let mut ids = std::collections::BTreeMap::new();
    for project in ["project", "other"] {
        let (status, item) = call(
            &app,
            "POST",
            "/inbox",
            json!({"request_key":project,"project_id":project,"title":"Idea"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        ids.insert(project, item["id"].as_str().unwrap().to_owned());
    }
    state
        .tasks
        .change(|board| {
            let mut first = task();
            first.inbox_id = ids["project"].clone();
            let mut second = task();
            second.id = "other-task".into();
            second.inbox_id = ids["other"].clone();
            second.project_id = "other".into();
            board.tasks = vec![first, second];
            Ok(())
        })
        .unwrap();
    for (project, task_id) in [("project", "task-1"), ("other", "other-task")] {
        let (status, board) =
            call(&app, "GET", &format!("/?project_id={project}"), Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let (_, inbox) = call(
            &app,
            "GET",
            &format!("/inbox?project_id={project}&include_converted=true"),
            Value::Null,
        )
        .await;
        assert_eq!(inbox["items"].as_array().unwrap().len(), 1);
        assert_eq!(inbox["items"][0]["id"], ids[project]);
        assert!(board.get("inbox").is_none());
        assert_eq!(board["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(board["tasks"][0]["id"], task_id);
        assert_eq!(board["statuses"].as_array().unwrap().len(), 4);
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/inbox",
            json!({"request_key":"project","project_id":"other","title":"Idea"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let path = format!("/inbox/{}", ids["project"]);
    assert_eq!(
        call(
            &app,
            "POST",
            &path,
            json!({"project_id":"other","title":"Idea","expected_revision":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(call(&app, "POST", &format!("{path}/convert"), json!({"request_key":"convert","expected_revision":1,"title":"Idea","status_id":"todo","project_id":"other","cwd":"/missing","agent":"codex"})).await.0, StatusCode::CONFLICT);
    assert_eq!(state.tasks.snapshot().unwrap().tasks.len(), 2);
    assert_eq!(state.tasks.get_inbox(&ids["project"]).unwrap().revision, 1);
}

#[tokio::test]
async fn inbox_creation_assigns_ids_deduplicates_retries_and_guards_edits() {
    let state = crate::AppState::with_terminald_socket("".into(), "/missing/socket".into());
    let app = routes().with_state(state.clone());
    let input = json!({"request_key":"capture-once", "project_id":"project", "title":"想法", "description":"待讨论"});
    let ((status, first), (retry_status, retry)) = tokio::join!(
        call(&app, "POST", "/inbox", input.clone()),
        call(&app, "POST", "/inbox", input.clone())
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retry_status, status);
    assert_eq!(retry, first);
    let id = first["id"].as_str().unwrap();
    assert!(aow_id::is_valid_id(id));
    assert_ne!(id, "capture-once");
    assert!(first.get("creation").is_none());
    assert!(first.get("request_key").is_none());
    assert!(state.tasks.snapshot().unwrap().tasks.is_empty());
    assert_eq!(
        state.tasks.inbox_page(None, true, 50, None).unwrap().total,
        1
    );
    let mut invalid = input.clone();
    invalid["id"] = json!("chosen-by-client");
    assert_eq!(
        call(&app, "POST", "/inbox", invalid).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut changed = input.clone();
    changed["title"] = json!("Different");
    assert_eq!(
        call(&app, "POST", "/inbox", changed).await.0,
        StatusCode::CONFLICT
    );
    let path = format!("/inbox/{id}");
    let (status, item) = call(
        &app,
        "POST",
        &path,
        json!({"project_id":"project", "expected_revision":1,"title":"Updated"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(item["revision"], 2);
    assert_eq!(
        call(&app, "POST", "/inbox", input).await.1,
        item,
        "replaying creation must not undo edits"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{path}/delete"),
            json!({"expected_revision":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{path}/delete"),
            json!({"expected_revision":2})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        state
            .tasks
            .inbox_page(None, true, 50, None)
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn arbitrary_statuses_support_backward_moves_without_execution_and_reject_stale_writes() {
    let state = crate::AppState::new("".into());
    let app = routes().with_state(state.clone());
    let (_, created) = call(&app, "POST", "/statuses", json!({"request_key":"statuses-first","expected_revision":1,"statuses":[{"name":"草图","color":"#123456"},{"name":"验收","color":"#abcdef"}]})).await;
    let statuses = created["statuses"].clone();
    let sketch = statuses[0]["id"].as_str().unwrap();
    let accepted = statuses[1]["id"].as_str().unwrap();
    state
        .tasks
        .change(|board| {
            let mut task = task();
            task.status_id = sketch.into();
            board.tasks.push(task);
            Ok(())
        })
        .unwrap();
    let (status, moved) = call(
        &app,
        "POST",
        "/items/task-1/status",
        json!({"expected_revision":1,"status_id":accepted,"reason":"Human review"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["execution"], "ready");
    assert_eq!(moved["pane_id"], Value::Null);
    assert_eq!(moved["history"][0]["reason"], "Human review");
    assert_eq!(
        call(
            &app,
            "POST",
            "/items/task-1/status",
            json!({"expected_revision":1,"status_id":sketch})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/items/task-1/status",
            json!({"expected_revision":2,"status_id":"done"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/statuses",
            json!({"request_key":"remove-used","expected_revision":2,"statuses":[statuses[0]]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, moved) = call(
        &app,
        "POST",
        "/items/task-1/status",
        json!({"expected_revision":2,"status_id":sketch}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["execution"], "ready");
    assert_eq!(moved["history"].as_array().unwrap().len(), 2);
    assert_eq!(
        call(
            &app,
            "POST",
            "/items/task-1/start",
            json!({"expected_revision":3})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[test]
fn persistence_recovers_interrupted_execution_and_failed_save_does_not_publish() {
    let directory = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigRepository::initialize(directory.path()).unwrap();
    let runtime = directory
        .path()
        .join("tasks")
        .join(config.selection().config_id)
        .join("tasks.json");
    let store = TaskStore::new(
        Some(directory.path()),
        Some(config.clone()),
        crate::workspace_events::WorkspaceEvents::new(),
    )
    .unwrap();
    assert!(!runtime.clone().exists());
    store
        .change(|board| {
            let mut task = task();
            task.execution = TaskExecutionPhase::Submitting;
            board.tasks.push(task);
            Ok(())
        })
        .unwrap();
    let restored = TaskStore::new(
        Some(directory.path()),
        Some(config.clone()),
        crate::workspace_events::WorkspaceEvents::new(),
    )
    .unwrap();
    let task = restored.get("task-1").unwrap();
    assert_eq!(task.execution, TaskExecutionPhase::Failed);
    assert!(task.error.unwrap().contains("delivery may have occurred"));
    // A directory at the target path causes atomic rename to fail even under root.
    std::fs::remove_file(runtime.clone()).unwrap();
    std::fs::create_dir(runtime.clone()).unwrap();
    assert!(
        restored
            .change(|board| {
                board.tasks.clear();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(restored.snapshot().unwrap().tasks.len(), 1);
}

#[tokio::test]
async fn web_and_local_cli_share_durable_task_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = crate::AppState::new("".into());
    let cli = crate::start_local_cli(state.clone(), directory.path())
        .await
        .unwrap();
    let client = aow_terminald_client::TerminaldClient::new(directory.path().join("cli/cli.sock"));
    let item: Value = client
        .post_json(
            "/v1/tasks/inbox",
            &json!({"request_key":"capture","project_id":"project","title":"From CLI"}),
        )
        .await
        .unwrap();
    let web = crate::build_router(state);
    let (status, board) = call(&web, "GET", "/api/tasks", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let (_, fetched) = call(
        &web,
        "GET",
        &format!("/api/tasks/inbox/{}", item["id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(fetched, item);
    let (_, inbox) = call(&web, "GET", "/api/tasks/inbox", Value::Null).await;
    assert_eq!(
        client.get_json::<Value>("/v1/tasks/inbox").await.unwrap(),
        inbox
    );
    assert_eq!(client.get_json::<Value>("/v1/tasks").await.unwrap(), board);
    cli.shutdown().await;
}

#[tokio::test]
async fn shared_status_configuration_allocates_ids_and_deduplicates_saves() {
    let state = crate::AppState::new("".into());
    let app = routes().with_state(state.clone());
    let input = json!({"request_key":"statuses-first","expected_revision":1,"statuses":[
        {"name":"等待确认","color":"#123456"}, {"name":"构思","color":"#abcdef"}
    ]});
    let (code, board) = call(&app, "POST", "/statuses", input.clone()).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(board["status_revision"], 2);
    let statuses = board["statuses"].clone();
    for status in statuses.as_array().unwrap() {
        assert!(aow_id::is_valid_id(status["id"].as_str().unwrap()));
    }
    assert_ne!(statuses[0]["id"], statuses[1]["id"]);
    assert_eq!(
        call(&app, "POST", "/statuses", input.clone()).await.1,
        board
    );
    let mut changed = input;
    changed["statuses"][0]["name"] = json!("Changed");
    assert_eq!(
        call(&app, "POST", "/statuses", changed).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/statuses",
            json!({"request_key":"stale","expected_revision":1,"statuses":statuses})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for invalid in [
        json!([]),
        json!([statuses[0], statuses[0]]),
        json!([{"name":"Bad","color":"red"}]),
        json!([{"id":"client-chosen","name":"Bad","color":"#123456"}]),
    ] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/statuses",
                json!({"request_key":"invalid","expected_revision":2,"statuses":invalid})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        serde_json::to_value(state.tasks.snapshot().unwrap()).unwrap(),
        board
    );
}
