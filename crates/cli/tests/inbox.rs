mod support;

use serde_json::{Value, json};
use std::process::Command;

fn value(output: &std::process::Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn requirements_and_comments_queries_use_the_same_local_instance() {
    for (args, path, reply, expected) in [
        (
            vec!["inbox", "list"],
            "/v1/inbox",
            json!({"items":[], "labels":[]}),
            json!({"items":[], "labels":[]}),
        ),
        (
            vec!["inbox", "get", "requirement-one"],
            "/v1/inbox/items/requirement-one",
            json!({"id":"requirement-one", "revision":1}),
            json!({"id":"requirement-one", "revision":1}),
        ),
        (
            vec!["inbox", "comments", "list", "requirement-one"],
            "/v1/inbox/items/requirement-one/comments",
            json!([{"id":"comment-one", "content":"正文"}]),
            json!({"items":[{"id":"comment-one", "content":"正文"}]}),
        ),
    ] {
        let (output, header, body) = support::run(&args, None, 200, reply);
        assert!(header.starts_with(&format!("GET {path} HTTP/1.1")));
        assert!(body.is_null());
        assert_eq!(value(&output), expected);
    }
}

#[test]
fn creates_and_comments_preserve_multiline_markdown_and_explicit_request_keys() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("description with spaces.md");
    let text = "# 标题\n\n**重点** & <literal>\n\n```rust\nfn main() {}\n```";
    std::fs::write(&file, text).unwrap();
    for source in [file.to_str().unwrap(), "-"] {
        let (output, header, body) = support::run(
            &[
                "inbox",
                "create",
                "--file",
                source,
                "--request-key",
                "capture-one",
            ],
            (source == "-").then_some(text),
            200,
            json!({"id":"requirement-one"}),
        );
        assert!(header.starts_with("POST /v1/inbox/items HTTP/1.1"));
        assert_eq!(body, json!({"markdown":text, "request_key":"capture-one"}));
        assert_eq!(value(&output)["id"], "requirement-one");
        let (output, header, body) = support::run(
            &[
                "inbox",
                "comments",
                "add",
                "requirement-one",
                "--file",
                source,
                "--author-type",
                "ai",
                "--author",
                "Custom Codex",
                "--request-key",
                "comment-one",
                "--compact",
            ],
            (source == "-").then_some(text),
            200,
            json!({"id":"comment-one"}),
        );
        assert!(header.starts_with("POST /v1/inbox/items/requirement-one/comments HTTP/1.1"));
        assert_eq!(
            body,
            json!({"content":text, "request_key":"comment-one", "author":{"type":"ai", "name":"Custom Codex"}})
        );
        assert_eq!(value(&output)["id"], "comment-one");
        assert_eq!(
            std::str::from_utf8(&output.stdout).unwrap().lines().count(),
            1
        );
    }
    let (output, _, body) = support::run(
        &["inbox", "create", "--content", text],
        None,
        200,
        json!({"id":"created"}),
    );
    assert_eq!(body["markdown"], text);
    assert!(aow_id::is_valid_id(body["request_key"].as_str().unwrap()));
    value(&output);
}

#[test]
fn updates_preserve_unspecified_fields_and_send_the_callers_expected_revision() {
    let current = json!({"markdown":"原始正文", "project_id":"project-one", "label_ids":["todo","review"], "revision":8});
    for (changes, markdown, project, labels) in [
        (
            vec!["--content", "修正后的正文"],
            "修正后的正文",
            json!("project-one"),
            json!(["todo", "review"]),
        ),
        (
            vec!["--unbound", "--clear-labels"],
            "原始正文",
            Value::Null,
            json!([]),
        ),
        (
            vec![
                "--project-id",
                "project-two",
                "--label-id",
                "done",
                "--label-id",
                "review",
            ],
            "原始正文",
            json!("project-two"),
            json!(["done", "review"]),
        ),
    ] {
        let mut args = vec![
            "inbox",
            "update",
            "requirement-one",
            "--expected-revision",
            "8",
        ];
        args.extend(changes);
        let (output, requests) = support::run_many(
            &args,
            None,
            vec![(200, current.clone()), (200, json!({"revision":9}))],
        );
        assert!(
            requests[0]
                .0
                .starts_with("GET /v1/inbox/items/requirement-one HTTP/1.1")
        );
        assert!(
            requests[1]
                .0
                .starts_with("PUT /v1/inbox/items/requirement-one HTTP/1.1")
        );
        assert_eq!(
            requests[1].1,
            json!({"expected_revision":8,"markdown":markdown,"project_id":project,"label_ids":labels})
        );
        assert_eq!(value(&output)["revision"], 9);
    }
    let (output, requests) = support::run_many(
        &[
            "inbox",
            "update",
            "requirement-one",
            "--expected-revision",
            "7",
            "--content",
            "旧修改",
        ],
        None,
        vec![
            (200, current),
            (409, json!({"message":"revision conflict"})),
        ],
    );
    assert_eq!(requests[1].1["expected_revision"], 7);
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
}

#[test]
fn deletion_sends_a_revision_and_returns_a_json_receipt() {
    let (output, header, body) = support::run(
        &[
            "inbox",
            "delete",
            "requirement-one",
            "--expected-revision",
            "3",
        ],
        None,
        204,
        Value::Null,
    );
    assert!(header.starts_with("DELETE /v1/inbox/items/requirement-one HTTP/1.1"));
    assert_eq!(body, json!({"expected_revision":3}));
    assert_eq!(
        value(&output),
        json!({"id":"requirement-one","deleted":true})
    );
}

#[test]
fn help_invalid_inputs_and_service_failures_never_initialize_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("absent-state");
    for args in [
        vec!["inbox", "--help"],
        vec!["inbox", "create", "--help"],
        vec!["inbox", "update", "--help"],
        vec!["inbox", "comments", "add", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains("Comments are immutable")
        );
    }
    for args in [
        vec!["inbox", "get", "../escape"],
        vec!["inbox", "create"],
        vec!["inbox", "create", "--content", " "],
        vec!["inbox", "create", "--content", "正文", "--file", "-"],
        vec![
            "inbox",
            "update",
            "requirement-one",
            "--expected-revision",
            "1",
        ],
        vec!["inbox", "update", "requirement-one", "--content", "正文"],
        vec![
            "inbox",
            "update",
            "requirement-one",
            "--expected-revision",
            "1",
            "--project-id",
            "project",
            "--unbound",
        ],
        vec![
            "inbox",
            "comments",
            "add",
            "requirement-one",
            "--content",
            "评论",
            "--author",
            " ",
        ],
        vec!["inbox", "comments", "edit", "requirement-one"],
        vec!["inbox", "comments", "delete", "requirement-one"],
        vec!["inbox", "delete", "requirement-one"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let file = directory.path().join("invalid.md");
    for bytes in [
        vec![b'x'; 128 * 1024 + 1],
        vec![0xff],
        b"text\0text".to_vec(),
    ] {
        std::fs::write(&file, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .arg("--state-dir")
            .arg(&state)
            .args(["inbox", "create", "--file"])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .arg("--state-dir")
        .arg(&state)
        .args(["inbox", "list"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!state.exists());
}

#[test]
fn remote_errors_use_standard_exit_codes_and_include_creation_retry_keys() {
    for (status, exit) in [(400, 2), (404, 3), (409, 7), (503, 1)] {
        let (output, _, _) = support::run(
            &[
                "inbox",
                "comments",
                "add",
                "requirement-one",
                "--content",
                "结论",
                "--author",
                "用户",
                "--request-key",
                "result-one",
            ],
            None,
            status,
            json!({"message":"failed"}),
        );
        assert_eq!(output.status.code(), Some(exit));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--request-key result-one")
        );
    }
}
