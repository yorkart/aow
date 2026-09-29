mod support;
use serde_json::{Value, json};
use support::run;

#[test]
fn statuses_use_explicit_revision_and_preserve_custom_ids() {
    let (output, header, body) = run(
        &[
            "task",
            "set-status",
            "task-123",
            "--status",
            "awaiting-acceptance",
            "--expected-revision",
            "7",
            "--reason",
            "PR is green",
        ],
        None,
        200,
        json!({"id":"task-123","revision":8}),
    );
    assert!(output.status.success());
    assert!(header.starts_with("POST /v1/tasks/items/task-123/status HTTP/1.1"));
    assert_eq!(
        body,
        json!({"expected_revision":7,"status_id":"awaiting-acceptance","reason":"PR is green"})
    );
    let (output, _, _) = run(
        &[
            "task",
            "set-status",
            "task-123",
            "--status",
            "done",
            "--expected-revision",
            "7",
        ],
        None,
        409,
        json!({"message":"stale task"}),
    );
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
}
#[test]
fn capture_does_not_send_agent_or_execution_configuration() {
    let (output, header, body) = run(
        &[
            "task",
            "capture",
            "--id",
            "idea-123",
            "--project-id",
            "project",
            "--title",
            "An idea",
        ],
        None,
        200,
        json!({"id":"idea-123","revision":1}),
    );
    assert!(output.status.success());
    assert!(header.starts_with("POST /v1/tasks/inbox HTTP/1.1"));
    assert_eq!(
        body,
        json!({"id":"idea-123","project_id":"project","expected_revision":null,"title":"An idea","description":""})
    );
}
#[test]
fn lists_read_the_shared_board_and_status_revision() {
    for (command, key) in [("list", "tasks"), ("statuses", "statuses")] {
        let expected = json!({"tasks":[{"id":"task"}],"statuses":[{"id":"todo"}],"status_revision":3,"inbox":[{"id":"idea"}]});
        let (output, header, body) = run(&["task", command], None, 200, expected.clone());
        assert!(output.status.success());
        assert!(header.starts_with("GET /v1/tasks HTTP/1.1"));
        assert_eq!(body, Value::Null);
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            if key == "statuses" {
                json!({"items":expected[key],"revision":3})
            } else {
                json!({"items":expected[key]})
            }
        );
    }
}

#[test]
fn create_uses_a_board_status_without_a_group() {
    let (output, header, body) = run(
        &[
            "task",
            "create",
            "--id",
            "task-123",
            "--inbox",
            "idea-123",
            "--expected-revision",
            "1",
            "--title",
            "An idea",
            "--project-id",
            "project",
            "--cwd",
            "/repo",
            "--status",
            "awaiting-acceptance",
        ],
        None,
        200,
        json!({"id":"task-123","revision":1}),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(header.starts_with("POST /v1/tasks/inbox/idea-123/convert HTTP/1.1"));
    assert_eq!(body["status_id"], "awaiting-acceptance");
    assert_eq!(body["start_now"], false);
    assert!(body.get("group_id").is_none());
}

#[test]
fn list_can_filter_by_project() {
    let (output, header, _) = run(
        &["task", "list", "--project-id", "project"],
        None,
        200,
        json!({"tasks":[]}),
    );
    assert!(output.status.success());
    assert!(header.starts_with("GET /v1/tasks?project_id=project HTTP/1.1"));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"items":[]})
    );
}

#[test]
fn inbox_pages_preserve_cursor_and_details_are_requested_separately() {
    let response = json!({"items":[{"id":"idea"}],"total":201,"next_cursor":"123_idea"});
    let (output, header, _) = run(
        &[
            "task",
            "inbox",
            "--project-id",
            "project",
            "--limit",
            "20",
            "--cursor",
            "456_previous",
            "--include-converted",
        ],
        None,
        200,
        response.clone(),
    );
    assert!(output.status.success());
    assert!(header.starts_with("GET /v1/tasks/inbox?limit=20&include_converted=true&project_id=project&cursor=456_previous HTTP/1.1"));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        response
    );
    let detail = json!({"id":"idea","description":"# Markdown"});
    let (output, header, _) = run(&["task", "inbox-get", "idea"], None, 200, detail.clone());
    assert!(output.status.success());
    assert!(header.starts_with("GET /v1/tasks/inbox/idea HTTP/1.1"));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        detail
    );
}
