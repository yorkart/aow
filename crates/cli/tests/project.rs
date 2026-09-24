mod support;

use serde_json::{Value, json};
use std::process::Command;
use support::run;

#[test]
fn project_queries_use_get_and_return_registry_metadata() {
    let project = json!({"id":"project-one", "name":"项目", "repo_path":"/repo with spaces"});
    for (args, path, expected) in [
        (
            vec!["project", "list"],
            "/v1/projects",
            json!({"items":[project.clone()]}),
        ),
        (
            vec!["project", "list", "--compact"],
            "/v1/projects",
            json!({"items":[]}),
        ),
        (
            vec!["project", "get", "project-one", "--compact"],
            "/v1/projects/project-one",
            project,
        ),
    ] {
        let (output, header, body) = run(&args, None, 200, expected.clone());
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert!(header.starts_with(&format!("GET {path} HTTP/1.1")));
        assert_eq!(body, Value::Null);
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            expected
        );
        if args.contains(&"--compact") {
            assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
        }
    }
}

#[test]
fn project_query_errors_are_distinct_from_an_empty_registry() {
    let (output, _, _) = run(
        &["project", "get", "missing"],
        None,
        404,
        json!({"message":"project not found"}),
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "not_found"
    );

    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .arg("--state-dir")
        .arg(&missing)
        .args(["project", "list"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "server_unavailable"
    );
    assert!(!missing.exists());
}

#[test]
fn project_help_and_invalid_arguments_do_not_initialize_state() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    for args in [
        vec!["project", "--help"],
        vec!["project", "list", "--help"],
        vec!["project", "get", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&missing)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("repo_path"));
        assert!(help.contains("git -C /repo worktree list --porcelain"));
    }
    for args in [
        vec!["project", "get"],
        vec!["project", "get", "../escape"],
        vec!["project", "create"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&missing)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
    assert!(!missing.exists());
}
