mod support;

use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn error(output: Output, status: i32, code: &str) {
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    assert!(output.stdout.is_empty());
    let body: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(body["error"]["code"], code);
    assert!(!body["error"]["message"].as_str().unwrap().is_empty());
}

#[test]
fn file_and_stdin_send_saved_configuration_to_the_local_server() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved task.json");
    let saved = json!({
        "id":"12345678", "name":"每日检查", "prompt":"检查项目\n保留完整内容",
        "agent":"codex", "project_id":"old-project", "workspace_path":"/old/repo",
        "workspace_mode":"existing", "cron":"0 9 * * *", "enabled":true,
        "max_concurrent_runs":2, "launch":{"executable":"/old/codex"}
    });
    let source = serde_json::to_string_pretty(&saved).unwrap();
    std::fs::write(&path, &source).unwrap();
    let created = json!({"id":"87654321", "enabled":false, "project_id":"target-project"});
    for file in [path.to_str().unwrap(), "-"] {
        let (output, header, request) = support::run(
            &[
                "automation",
                "create",
                "--file",
                file,
                "--project-id",
                "target-project",
                "--compact",
            ],
            (file == "-").then_some(source.as_str()),
            201,
            created.clone(),
        );
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert!(header.starts_with("POST /v1/automations HTTP/1.1"));
        assert_eq!(
            request,
            json!({"project_id":"target-project", "configuration":saved})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            created
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
    }
}

#[test]
fn invalid_files_and_arguments_do_not_initialize_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("absent-state");
    let path = directory.path().join("task.json");
    let args = [
        "automation",
        "create",
        "--project-id",
        "target-project",
        "--file",
        path.to_str().unwrap(),
    ];
    for source in [
        String::new(),
        "{".into(),
        "[]".into(),
        "null".into(),
        "{} {}".into(),
        " ".repeat(1024 * 1024 + 1),
    ] {
        std::fs::write(&path, source).unwrap();
        error(
            Command::new(env!("CARGO_BIN_EXE_aow-cli"))
                .arg("--state-dir")
                .arg(&state)
                .args(args)
                .output()
                .unwrap(),
            2,
            "invalid_arguments",
        );
    }
    std::fs::remove_file(&path).unwrap();
    error(
        Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .output()
            .unwrap(),
        3,
        "not_found",
    );
    for args in [
        vec!["automation", "create"],
        vec!["automation", "create", "--file", "-"],
        vec!["automation", "create", "--project-id", "project"],
        vec![
            "automation",
            "create",
            "--project-id",
            "../escape",
            "--file",
            "-",
        ],
    ] {
        error(
            Command::new(env!("CARGO_BIN_EXE_aow-cli"))
                .arg("--state-dir")
                .arg(&state)
                .args(args)
                .output()
                .unwrap(),
            2,
            "invalid_arguments",
        );
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .arg("--state-dir")
        .arg(&state)
        .args([
            "automation",
            "create",
            "--file",
            "-",
            "--project-id",
            "project",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not JSON").unwrap();
    error(child.wait_with_output().unwrap(), 2, "invalid_arguments");
    let help = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .arg("--state-dir")
        .arg(&state)
        .args(["automation", "create", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("enabled=false"));
    assert!(help.contains("--file -"));
    assert!(!state.exists());
}

#[test]
fn creation_errors_have_nonzero_exit_codes_and_no_success_output() {
    for (status, exit_code, code) in [
        (400, 2, "invalid_arguments"),
        (404, 3, "not_found"),
        (503, 1, "server_unavailable"),
    ] {
        let (output, _, _) = support::run(
            &[
                "automation",
                "create",
                "--file",
                "-",
                "--project-id",
                "project",
            ],
            Some("{}"),
            status,
            json!({"message":"Cannot create automation"}),
        );
        error(output, exit_code, code);
    }
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("no-server");
    let file = directory.path().join("task.json");
    std::fs::write(&file, "{}").unwrap();
    error(
        Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&state)
            .args([
                "automation",
                "create",
                "--file",
                file.to_str().unwrap(),
                "--project-id",
                "project",
            ])
            .output()
            .unwrap(),
        1,
        "server_unavailable",
    );
    assert!(!state.exists());
}
