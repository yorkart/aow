mod support;

use serde_json::{Value, json};
use support::run;

#[test]
fn role_discovery_and_task_results_preserve_scope() {
    for (args, path) in [
        (vec!["role", "list"], "/v1/roles"),
        (vec!["role", "get", "reviewer"], "/v1/roles/reviewer"),
        (
            vec!["task", "list", "--source-thread", "thread-a"],
            "/v1/tasks?source_thread_id=thread-a",
        ),
        (
            vec!["task", "get", "task-b", "--source-thread", "thread-a"],
            "/v1/tasks/task-b?source_thread_id=thread-a",
        ),
    ] {
        let (output, header, body) = run(&args, None, 200, json!({"items":[]}));
        assert!(output.status.success(), "{output:?}");
        assert!(header.starts_with(&format!("GET {path} HTTP/1.1")));
        assert_eq!(body, Value::Null);
    }
}

#[test]
fn task_creation_preserves_multiline_input_and_idempotency_identity() {
    let text = "审查登录功能\nKeep `literal` and $(text) intact.";
    let (output, header, body) = run(
        &[
            "task",
            "create",
            "--source-thread",
            "thread-a",
            "--request-id",
            "stable-request",
            "--role",
            "reviewer",
            "--intent",
            "review",
            "--target-task",
            "implementation",
            "--task-file",
            "-",
        ],
        Some(text),
        200,
        json!({"id":"stable-request"}),
    );
    assert!(output.status.success(), "{output:?}");
    assert!(header.starts_with("POST /v1/tasks HTTP/1.1"));
    assert_eq!(
        body,
        json!({"source_thread_id":"thread-a","request_id":"stable-request","agent_role_id":"reviewer",
        "intent":"review","target_task_id":"implementation","prompt":text})
    );
}

#[test]
fn continuing_a_busy_task_returns_the_message_receipt_without_waiting() {
    let (output, header, body) = run(
        &[
            "task",
            "send",
            "task-b",
            "--source-thread",
            "thread-a",
            "--request-id",
            "stable-send",
            "--task",
            "Continue",
        ],
        None,
        200,
        json!({"accepted":true,"message_id":"message-b","delivery":"pending"}),
    );
    assert!(header.starts_with("POST /v1/tasks/task-b/send HTTP/1.1"));
    assert_eq!(body["source_thread_id"], "thread-a");
    assert_eq!(body["request_id"], "stable-send");
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["message_id"], "message-b");
}

#[test]
fn explicit_reports_and_task_status_are_independent_commands() {
    let text = "POC verified\nKeep `literal` and $(text) intact.";
    let (output, header, body) = run(
        &[
            "task",
            "report",
            "login",
            "--source-thread",
            "developer",
            "--request-id",
            "poc-report",
            "--kind",
            "progress",
            "--target-thread",
            "parent",
            "--task-file",
            "-",
        ],
        Some(text),
        200,
        json!({"id":"event"}),
    );
    assert!(output.status.success(), "{output:?}");
    assert!(header.starts_with("POST /v1/tasks/login/report HTTP/1.1"));
    assert_eq!(
        body,
        json!({"source_thread_id":"developer","request_id":"poc-report","kind":"progress","target_thread_id":"parent","message":text})
    );
    let (output, header, body) = run(
        &[
            "task",
            "status",
            "login",
            "done",
            "--source-thread",
            "router",
        ],
        None,
        200,
        json!({"status":"done"}),
    );
    assert!(output.status.success(), "{output:?}");
    assert!(header.starts_with("POST /v1/tasks/login/status HTTP/1.1"));
    assert_eq!(body, json!({"status":"done", "source_thread_id":"router"}));
}
