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
fn continuing_a_busy_task_reports_conflict_without_reissuing_the_message() {
    let (output, header, body) = run(
        &[
            "task",
            "send",
            "task-b",
            "--source-thread",
            "thread-a",
            "--task",
            "Continue",
        ],
        None,
        409,
        json!({"message":"ACP session is busy"}),
    );
    assert!(header.starts_with("POST /v1/tasks/task-b/send HTTP/1.1"));
    assert_eq!(body["source_thread_id"], "thread-a");
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
}
