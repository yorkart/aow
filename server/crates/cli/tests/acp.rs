mod support;
use serde_json::{Value, json};

#[test]
fn both_creations_transmit_native_session_context_without_changing_task() {
    for command in ["agent", "acp"] {
        let reply = if command == "agent" {
            json!({"pane_id":"child","tab_id":"tab","cwd":"/repo","agent":"test","status":"running","phase":"ready","task_submitted":true,"error":null})
        } else {
            json!({"thread_id":"child","phase":"ready"})
        };
        let (output, _, body) = support::run_with_env(
            &[
                command,
                "create",
                "--agent-id",
                "test",
                "--project-id",
                "project",
                "--cwd",
                "/repo",
                "--session-id",
                "native/session:one",
                "--task",
                "exact user task",
            ],
            None,
            200,
            reply,
            &[
                ("AOW_SESSION_ID", "stale"),
                ("AOW_ACP_CONNECTION_ID", "connection-one"),
            ],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            body["caller"],
            json!({"session_id":"native/session:one", "acp_connection_id":"connection-one"})
        );
        assert!(body["parent"].is_null());
        assert_eq!(body["task"], "exact user task");
    }
}

#[test]
fn creation_requires_a_nonempty_caller_session_even_with_an_explicit_parent() {
    for command in ["agent", "acp"] {
        for value in [None, Some(""), Some(" \n")] {
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_aow-cli"));
            cmd.env_remove("AOW_SESSION_ID")
                .env_remove("AOW_ACP_CONNECTION_ID")
                .args([
                    command,
                    "create",
                    "--agent-id",
                    "test",
                    "--project-id",
                    "project",
                    "--cwd",
                    "/repo",
                    "--parent",
                    "terminal:parent",
                ]);
            if let Some(value) = value {
                cmd.args(["--session-id", value]);
            }
            let output = cmd.output().unwrap();
            assert!(!output.status.success());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("session")
            );
        }
    }
}

#[test]
fn explicit_and_inherited_parents_work_for_both_creation_commands() {
    for command in ["agent", "acp"] {
        for (environment, explicit, expected) in [
            (
                vec![],
                vec!["--parent", "acp:parent"],
                json!({"kind":"acp","id":"parent"}),
            ),
            (
                vec![("AOW_INSTANCE", "acp:current"), ("AOW_PANE_ID", "stale")],
                vec![],
                json!({"kind":"acp","id":"current"}),
            ),
            (
                vec![("AOW_INSTANCE", "acp:current")],
                vec!["--parent", "terminal:chosen"],
                json!({"kind":"terminal","id":"chosen"}),
            ),
            (
                vec![("AOW_INSTANCE", "terminal:current")],
                vec![],
                json!({"kind":"terminal","id":"current"}),
            ),
            (vec![("AOW_PANE_ID", "removed")], vec![], Value::Null),
            (
                vec![("AOW_INSTANCE", "invalid")],
                vec!["--no-parent"],
                Value::Null,
            ),
        ] {
            let mut args = vec![
                command,
                "create",
                "--agent-id",
                "test",
                "--project-id",
                "project",
                "--cwd",
                "/repo",
            ];
            args.extend(explicit);
            let reply = if command == "agent" {
                json!({"pane_id":"child","tab_id":"tab","cwd":"/repo","agent":"codex","status":"running","phase":"ready","task_submitted":false,"error":null})
            } else {
                json!({"thread_id":"child","phase":"ready"})
            };
            let (output, header, body) =
                support::run_with_env(&args, None, 200, reply, &environment);
            assert!(output.status.success(), "{output:?}");
            assert!(header.starts_with(if command == "agent" {
                "POST /v1/agents "
            } else {
                "POST /v1/acp/sessions "
            }));
            assert_eq!(body["parent"], expected);
            assert_eq!(body["caller"]["session_id"], "fixture-caller");
            assert!(body.get("parent_pane_id").is_none());
        }
    }
}

#[test]
fn acp_tasks_failures_and_queries_preserve_identity() {
    let (output, _, body) = support::run(
        &[
            "acp",
            "create",
            "--agent-id",
            "test",
            "--project-id",
            "project",
            "--cwd",
            "/repo",
            "--task-file",
            "-",
        ],
        Some("first\nsecond"),
        200,
        json!({"thread_id":"durable", "phase":"failed", "startup_error":"login required"}),
    );
    assert_eq!(body["task"], "first\nsecond");
    assert_eq!(output.status.code(), Some(6));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["thread_id"], "durable");
    for (args, path) in [
        (vec!["acp", "list"], "/v1/acp/sessions"),
        (vec!["acp", "agents"], "/v1/acp/agents"),
        (
            vec!["acp", "get", "--thread-id", "child"],
            "/v1/acp/sessions/child",
        ),
        (
            vec!["instance", "get", "acp:child"],
            "/v1/instances/acp:child",
        ),
        (vec!["instance", "list"], "/v1/instances"),
    ] {
        let (output, header, _) = support::run(&args, None, 200, json!({"items":[]}));
        assert!(output.status.success());
        assert!(header.starts_with(&format!("GET {path} ")));
    }
    let (output, _, body) = support::run(
        &["acp", "submit", "--thread-id", "child", "--task", "next"],
        None,
        409,
        json!({"message":"owned"}),
    );
    assert_eq!(body["task"], "next");
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn invalid_parent_flags_and_help_do_not_initialize_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("missing");
    for command in ["agent", "acp"] {
        for flags in [
            vec!["--parent", "bad"],
            vec!["--parent", "acp:../escape"],
            vec!["--parent-pane-id", "two"],
            vec!["--parent", "acp:one", "--no-parent"],
        ] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_aow-cli"))
                .arg("--state-dir")
                .arg(&state)
                .args([
                    command,
                    "create",
                    "--agent-id",
                    "test",
                    "--project-id",
                    "project",
                    "--cwd",
                    "/repo",
                ])
                .args(flags)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
        }
    }
    for command in ["acp", "instance"] {
        assert!(
            std::process::Command::new(env!("CARGO_BIN_EXE_aow-cli"))
                .arg("--state-dir")
                .arg(&state)
                .args([command, "--help"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    assert!(!state.exists());
}

#[test]
fn omitted_profile_is_resolved_by_the_server_for_both_commands() {
    for command in ["agent", "acp"] {
        let reply = if command == "agent" {
            json!({"pane_id":"child","tab_id":"tab","cwd":"/repo","agent":"selected","status":"running","phase":"ready","task_submitted":false,"error":null})
        } else {
            json!({"thread_id":"child","phase":"ready"})
        };
        let (output, _, body) = support::run(
            &[
                command,
                "create",
                "--project-id",
                "project",
                "--cwd",
                "/repo",
            ],
            None,
            200,
            reply,
        );
        assert!(output.status.success(), "{output:?}");
        assert!(
            body.get(if command == "agent" {
                "agent"
            } else {
                "agent_id"
            })
            .is_none(),
            "{body}"
        );
    }
}
