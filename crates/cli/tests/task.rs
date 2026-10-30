mod support;
use serde_json::{Value, json};

#[test]
fn child_creation_infers_context_and_sends_only_task_options() {
    let (output, header, body) = support::run_with_env(
        &["task", "create", "--task", "Review", "--worktree", "new"],
        None,
        200,
        json!({"id":"child","phase":"ready","session_id":"native-child"}),
        &[("AOW_SESSION_ID", "native/session:parent")],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(header.starts_with("POST /v1/tasks "));
    assert_eq!(body["from_session_id"], "native/session:parent");
    assert_eq!(body["task"], "Review");
    assert_eq!(body["worktree"], "new");
    assert!(body["project_id"].is_null());
    assert!(body["agent_runtime_id"].is_null());
    assert!(body.get("parent_id").is_none());
    assert!(body["request_id"].as_str().is_some());
}
#[test]
fn explicit_session_overrides_environment_and_stdin_is_preserved() {
    let (output, _, body) = support::run_with_env(
        &[
            "task",
            "create",
            "--from-session-id",
            "explicit",
            "--task-file",
            "-",
        ],
        Some("Exact task\n"),
        200,
        json!({"id":"child","phase":"ready"}),
        &[("AOW_SESSION_ID", "old")],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(body["from_session_id"], "explicit");
    assert_eq!(body["task"], "Exact task\n");
    assert_eq!(body["worktree"], "current");
}
#[test]
fn list_encodes_native_identity_and_filters_and_get_needs_only_task_id() {
    let (output, header, _) = support::run_with_env(
        &[
            "task",
            "list",
            "--include-archived",
            "--parent-id",
            "parent",
        ],
        None,
        200,
        json!({"items":[]}),
        &[("AOW_SESSION_ID", "native/a?b&c")],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(header.contains("from_session_id=native%2Fa%3Fb%26c"));
    assert!(header.contains("include_archived=true"));
    assert!(header.contains("parent_id=parent"));
    let (output, header, _) = support::run(
        &["task", "get", "task-one"],
        None,
        200,
        json!({"id":"task-one","session_id":"native"}),
    );
    assert!(output.status.success());
    assert!(header.starts_with("GET /v1/tasks/task-one "));
}
#[test]
fn submit_archive_and_delete_use_task_identity() {
    for (args, suffix) in [
        (
            vec!["task", "submit", "one", "--task", "Continue"],
            "submit",
        ),
        (vec!["task", "archive", "one"], "archive"),
        (vec!["task", "delete", "one"], "delete"),
    ] {
        let (output, header, body) = support::run(&args, None, 200, json!({"id":"one"}));
        assert!(output.status.success(), "{output:?}");
        assert!(header.starts_with(&format!("POST /v1/tasks/one/{suffix} ")));
        if suffix == "submit" {
            assert_eq!(body["task"], "Continue");
        }
    }
}
#[test]
fn startup_failure_preserves_identity_and_service_errors_have_exit_codes() {
    let (output, _, _) = support::run(
        &["task", "create", "--task", "Review"],
        None,
        200,
        json!({"id":"created","thread_id":"thread","phase":"failed"}),
    );
    assert_eq!(output.status.code(), Some(6));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["task_id"], "created");
    for (status, code) in [(404, 3), (409, 7)] {
        let (output, _, _) = support::run(
            &["task", "get", "missing"],
            None,
            status,
            json!({"message":"Task unavailable"}),
        );
        assert_eq!(output.status.code(), Some(code));
    }
}
#[test]
fn task_help_has_all_lifecycle_commands_without_a_running_server() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .args(["task", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for name in ["create", "list", "get", "submit", "archive", "delete"] {
        assert!(help.contains(name));
    }
}

#[test]
fn explicit_project_creates_a_root_without_inheriting_unrelated_session_context() {
    let (output, _, body) = support::run(
        &[
            "task",
            "create",
            "--project-id",
            "project",
            "--task",
            "Root task",
        ],
        None,
        200,
        json!({"id":"root","phase":"ready"}),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(body["project_id"], "project");
    assert!(body["from_session_id"].is_null());
}
