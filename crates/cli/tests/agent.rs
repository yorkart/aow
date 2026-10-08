mod support;

use serde_json::{Value, json};
use std::process::Command;
use support::run;

#[test]
fn create_records_explicit_or_inherited_parent_and_returns_it() {
    for (inherited, explicit, expected) in [
        (None, None, None),
        (Some("parent-one"), None, Some("parent-one")),
        (None, Some("parent-two"), Some("parent-two")),
        (Some("parent-one"), Some("parent-two"), Some("parent-two")),
    ] {
        let mut args = vec![
            "agent",
            "create",
            "--project-id",
            "project-one",
            "--cwd",
            "/repo",
        ];
        if let Some(parent) = explicit {
            args.extend(["--parent-pane-id", parent]);
        }
        let environment: Vec<_> = inherited
            .map(|parent| ("AOW_PANE_ID", parent))
            .into_iter()
            .collect();
        let mut reply = info("ready");
        if let Some(parent) = expected {
            reply["parent_pane_id"] = json!(parent);
        }
        let (output, _, body) =
            support::run_with_env(&args, None, 200, reply.clone(), &environment);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(body["parent_pane_id"], json!(expected));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            reply
        );
    }
}

fn info(phase: &str) -> Value {
    json!({"pane_id":"pane-one", "tab_id":"tab-one", "cwd":"/repo", "agent":"codex",
        "status":"running", "phase":phase, "task_submitted":false,
        "error": if phase == "failed" { Some("startup timed out") } else { None }})
}

#[test]
fn create_reports_ready_and_preserves_failed_pane_identity() {
    for (phase, agent) in [
        ("ready", "codex"),
        ("failed", "codex"),
        ("ready", "hermes"),
        ("ready", "pi"),
    ] {
        let mut expected = info(phase);
        expected["agent"] = json!(agent);
        let (output, header, body) = run(
            &[
                "agent",
                "create",
                "--project-id",
                "project-one",
                "--agent",
                agent,
                "--cwd",
                "/repo",
                "--timeout",
                "7",
            ],
            None,
            200,
            expected.clone(),
        );
        assert!(header.starts_with("POST /v1/agents HTTP/1.1"));
        assert_eq!(body["timeout_seconds"], 7);
        assert_eq!(body["agent"], agent);
        assert_eq!(body["cwd"], "/repo");
        assert_eq!(body["project_id"], "project-one");
        for removed in ["repository", "branch", "base", "name"] {
            assert!(
                body.get(removed).is_none(),
                "unexpected field {removed}: {body}"
            );
        }
        if phase == "ready" {
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap(),
                expected
            );
        } else {
            assert_eq!(output.status.code(), Some(6));
            assert!(output.stdout.is_empty());
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["code"], "agent_not_ready");
            assert_eq!(error["error"]["pane_id"], "pane-one");
            assert_eq!(error["agent"], expected);
        }
    }
}

#[test]
fn submit_preserves_stdin_and_reports_control_conflicts() {
    let task = "执行第一步\nthen keep $HOME and `literal` unchanged\n";
    let (output, header, body) = run(
        &[
            "agent",
            "submit",
            "--pane-id",
            "pane-one",
            "--task-file",
            "-",
        ],
        Some(task),
        409,
        json!({"message":"pane is controlled by a user"}),
    );
    assert!(header.starts_with("POST /v1/agents/pane-one/submit HTTP/1.1"));
    assert_eq!(body, json!({"task":task}));
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "conflict"
    );
}

#[test]
fn agent_help_and_invalid_arguments_do_not_initialize_state() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    for args in [
        vec!["agent", "--help"],
        vec!["agent", "create", "--help"],
        vec!["agent", "submit", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&missing)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("pane_id"));
        for removed in ["--repo", "--branch", "--base", "--name"] {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(removed));
        }
    }
    for args in [
        vec![
            "agent",
            "create",
            "--project-id",
            "project",
            "--cwd",
            "/repo",
            "--name",
            "custom",
        ],
        vec!["agent", "create", "--timeout", "0"],
        vec!["agent", "create", "--agent", "unknown"],
        vec!["agent", "create", "--repo", "/repo"],
        vec!["agent", "create", "--project-id", "project"],
        vec!["agent", "create", "--cwd", "/repo"],
        vec!["agent", "create", "--branch", "feature"],
        vec!["agent", "create", "--base", "main"],
        vec![
            "agent",
            "submit",
            "--pane-id",
            "../escape",
            "--task",
            "task",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&missing)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    assert!(!missing.exists());
}
