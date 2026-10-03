use super::{InboxStore, model::*};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn capture(store: &InboxStore, key: &str, markdown: &str) -> Item {
    store
        .capture(Capture {
            request_key: key.into(),
            markdown: markdown.into(),
        })
        .unwrap()
}
fn update(item: &Item) -> Update {
    Update {
        expected_revision: item.revision,
        markdown: item.markdown.clone(),
        project_id: item.project_id.clone(),
        label_ids: item.label_ids.clone(),
    }
}

#[test]
fn raw_markdown_capture_is_project_optional_and_idempotent() {
    let store = InboxStore::in_memory();
    let markdown = "# 标题\n\n- [ ] 内容 `literal`\n\n```rust\nfn main() {}\n```\n";
    let first = capture(&store, "capture-one", markdown);
    let retry = capture(&store, "capture-one", markdown);
    assert_eq!(first.id, retry.id);
    assert_eq!(first.markdown, markdown);
    assert!(first.project_id.is_none());
    assert!(first.label_ids.is_empty());
    assert_eq!(store.snapshot().unwrap().items.len(), 1);
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .labels
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>(),
        ["TODO", "InProgress", "Review", "Done"]
    );
    assert_eq!(
        store
            .capture(Capture {
                request_key: "capture-one".into(),
                markdown: "different".into()
            })
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    for markdown in [
        "   ".to_string(),
        "bad\0text".to_string(),
        "x".repeat(128 * 1024 + 1),
    ] {
        assert!(
            store
                .capture(Capture {
                    request_key: "invalid".into(),
                    markdown
                })
                .is_err()
        );
    }
}

#[test]
fn edits_conflict_without_overwriting_and_metadata_does_not_change_markdown() {
    let store = InboxStore::in_memory();
    let first = capture(&store, "first", "A\n\nB");
    let mut input = update(&first);
    input.project_id = Some("project-a".into());
    input.label_ids = vec!["todo".into(), "review".into()];
    let saved = store.update(&first.id, input).unwrap();
    assert_eq!(saved.markdown, first.markdown);
    assert_eq!(saved.revision, 2);
    assert_eq!(
        store.update(&first.id, update(&first)).unwrap_err().status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        store.delete(&first.id, first.revision).unwrap_err().status,
        StatusCode::CONFLICT
    );
    let mut invalid = update(&saved);
    invalid.label_ids = vec!["missing".into()];
    assert!(store.update(&first.id, invalid).is_err());
    assert_eq!(store.item(&first.id).unwrap().revision, saved.revision);
    store.delete(&saved.id, saved.revision).unwrap();
    assert!(store.snapshot().unwrap().items.is_empty());
    assert!(
        store
            .capture(Capture {
                request_key: "first".into(),
                markdown: first.markdown
            })
            .is_err()
    );
}

#[test]
fn reorder_moves_one_item_while_retaining_other_projects_and_rejects_stale_orders() {
    let store = InboxStore::in_memory();
    let a = capture(&store, "a", "A");
    let b = capture(&store, "b", "B");
    let c = capture(&store, "c", "C");
    let snapshot = store.snapshot().unwrap();
    store
        .reorder(Reorder {
            expected_revision: snapshot.revision,
            item_id: c.id.clone(),
            before_id: Some(a.id.clone()),
        })
        .unwrap();
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .items
            .iter()
            .map(|i| &i.id)
            .collect::<Vec<_>>(),
        [&c.id, &a.id, &b.id]
    );
    assert_eq!(
        store
            .reorder(Reorder {
                expected_revision: snapshot.revision,
                item_id: b.id.clone(),
                before_id: None
            })
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    let revision = store.snapshot().unwrap().revision;
    assert!(
        store
            .reorder(Reorder {
                expected_revision: revision,
                item_id: b.id,
                before_id: Some("missing".into())
            })
            .is_err()
    );
    assert_eq!(store.snapshot().unwrap().revision, revision);
}

#[test]
fn custom_labels_remove_references_atomically_and_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let item = capture(&store, "a", "需求");
    let mut input = update(&item);
    input.label_ids = vec!["todo".into()];
    let item = store.update(&item.id, input).unwrap();
    let labels = vec![Label {
        id: "custom".into(),
        name: "Waiting".into(),
        color: "#123abc".into(),
    }];
    store
        .labels(LabelsUpdate {
            expected_revision: store.snapshot().unwrap().revision,
            labels,
        })
        .unwrap();
    let item = store.item(&item.id).unwrap();
    assert!(item.label_ids.is_empty());
    assert_eq!(item.revision, 3);
    let reopened = InboxStore::persistent(root.path()).unwrap();
    assert_eq!(reopened.snapshot().unwrap().labels[0].name, "Waiting");
    assert_eq!(reopened.item(&item.id).unwrap().markdown, "需求");
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(config.directory())
        .output()
        .unwrap();
    assert!(output.stdout.is_empty());
    std::fs::write(config.directory().join("inbox/labels.json"), b"corrupt").unwrap();
    assert!(InboxStore::persistent(root.path()).is_err());
}

#[test]
fn config_commit_failure_preserves_durable_capture_and_retry_does_not_duplicate() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let lock = config.directory().parent().unwrap().join(".git/index.lock");
    std::fs::write(&lock, "busy").unwrap();
    assert!(
        store
            .capture(Capture {
                request_key: "key".into(),
                markdown: "keep me".into()
            })
            .is_err()
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.items.len(), 1);
    std::fs::remove_file(lock).unwrap();
    let retry = capture(&store, "key", "keep me");
    assert_eq!(retry.id, snapshot.items[0].id);
    assert_eq!(
        InboxStore::persistent(root.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn executions_keep_input_snapshot_deduplicate_and_recover_without_resubmission() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let item = capture(&store, "a", "raw\n\nmarkdown");
    let mut input = update(&item);
    input.project_id = Some("project".into());
    let item = store.update(&item.id, input).unwrap();
    let request = Execute {
        expected_revision: item.revision,
        request_key: "launch".into(),
        agent: "codex".into(),
        append_prompt: "先分析后执行".into(),
        workspace: aow_workspaces::WorkspaceConfig {
            workspace_mode: aow_workspaces::WorkspaceMode::Existing,
            workspace_path: "/repo".into(),
            base_branch: String::new(),
        },
    };
    let markdown = request.final_task(&item.markdown);
    let fingerprint = request.fingerprint().unwrap();
    let (run, fresh) = store
        .begin_run(
            &item,
            &request,
            "/repo".into(),
            markdown.clone(),
            fingerprint.clone(),
        )
        .unwrap();
    assert!(fresh);
    let (retried, fresh) = store
        .begin_run(&item, &request, "/repo".into(), markdown, fingerprint)
        .unwrap();
    assert!(!fresh);
    assert_eq!(retried.id, run.id);
    assert!(
        retried
            .markdown
            .ends_with("\n\nraw\n\nmarkdown\n\n先分析后执行")
    );
    assert!(
        retried
            .markdown
            .contains(&format!("<requirement id=\"{}\" />", item.id))
    );
    assert!(
        retried
            .markdown
            .contains(&format!("<execution id=\"{}\" />", run.id))
    );
    assert_eq!(
        store
            .existing_run(
                &item.id,
                &Execute {
                    workspace: aow_workspaces::WorkspaceConfig {
                        base_branch: "another-branch".into(),
                        ..request.workspace.clone()
                    },
                    ..request.clone()
                },
            )
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    assert!(
        store
            .begin_run(
                &item,
                &Execute {
                    request_key: "second".into(),
                    ..request
                },
                "/repo".into(),
                String::new(),
                String::new(),
            )
            .is_err()
    );
    let mut edited = update(&item);
    edited.markdown = "changed after launching".into();
    store.update(&item.id, edited).unwrap();
    let reopened = InboxStore::persistent(root.path()).unwrap();
    let recovered = reopened.snapshot().unwrap().executions.remove(0);
    assert_eq!(recovered.phase, RunPhase::Interrupted);
    assert_eq!(recovered.markdown, retried.markdown);
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let saved = std::fs::read_to_string(config.directory().join("inbox/labels.json")).unwrap();
    assert!(!saved.contains("request_key"));
    assert!(!saved.contains("executions"));
}

#[test]
fn execution_context_is_strict_xml_with_only_three_nodes_and_preserves_task_text() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let requirement = "req<&\"'>";
    let execution = "run<&\"'>";
    let markdown = "# 原始需求\n\n<literal> & `code`\n\n补充 prompt";
    let task = super::context::task(requirement, execution, markdown);
    let (xml, original) = task.split_once("</aow-inbox>\n\n").unwrap();
    assert_eq!(original, markdown);
    let mut parser = Command::new("/usr/bin/python3")
        .args(["-c", "import sys,json,xml.etree.ElementTree as ET; root=ET.fromstring(sys.stdin.read()); print(json.dumps({'root':root.tag,'nodes':[node.tag for node in root],'requirement':root[0].attrib['id'],'execution':root[1].attrib['id'],'description':root[2].text}))"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    parser
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{xml}</aow-inbox>").as_bytes())
        .unwrap();
    let output = parser.wait_with_output().unwrap();
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["root"], "aow-inbox");
    assert_eq!(
        parsed["nodes"],
        json!(["requirement", "execution", "description"])
    );
    assert_eq!(parsed["requirement"], requirement);
    assert_eq!(parsed["execution"], execution);
    assert_eq!(
        parsed["description"],
        "This task comes from AoW Inbox. You can use aow-cli inbox commands to retrieve requirements and comments, update requirements, and append comments."
    );
}

#[test]
fn oversized_final_task_including_context_does_not_create_an_execution() {
    let store = InboxStore::in_memory();
    let mut item = capture(&store, "capture", "x".repeat(128 * 1024).as_str());
    let mut input = update(&item);
    input.project_id = Some("project".into());
    item = store.update(&item.id, input).unwrap();
    let execute = Execute {
        expected_revision: item.revision,
        request_key: "run".into(),
        agent: "codex".into(),
        append_prompt: String::new(),
        workspace: aow_workspaces::WorkspaceConfig {
            workspace_mode: aow_workspaces::WorkspaceMode::Existing,
            workspace_path: "/repo".into(),
            base_branch: String::new(),
        },
    };
    assert_eq!(
        store
            .begin_run(
                &item,
                &execute,
                "/repo".into(),
                item.markdown.clone(),
                execute.fingerprint().unwrap()
            )
            .unwrap_err()
            .status,
        StatusCode::BAD_REQUEST
    );
    assert!(store.snapshot().unwrap().executions.is_empty());
}

fn comment(key: &str, content: &str) -> AddComment {
    AddComment {
        request_key: key.into(),
        author: CommentAuthor {
            kind: AuthorType::Ai,
            name: "Codex".into(),
        },
        content: content.into(),
    }
}

#[test]
fn requirement_directories_keep_append_only_comments_and_are_moved_on_deletion() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let item = capture(&store, "requirement", "# 原始需求");
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let active = config
        .directory()
        .join("inbox/requirements/active")
        .join(&item.id);
    let deleted = config
        .directory()
        .join("inbox/requirements/deleted")
        .join(&item.id);
    assert_eq!(std::fs::read_dir(&active).unwrap().count(), 2);
    assert_eq!(std::fs::read(active.join("comments.jsonl")).unwrap(), b"");
    assert!(!config.directory().join("inbox.json").exists());
    let first = store
        .append_comment(
            &item.id,
            comment("analysis", "## 需求展开\n\n- **第一步**\n- 第二步"),
        )
        .unwrap();
    let prefix = std::fs::read(active.join("comments.jsonl")).unwrap();
    let second = store
        .append_comment(
            &item.id,
            comment("conclusion", "实现完成。\n\n```rust\nfn main() {}\n```"),
        )
        .unwrap();
    let retry = store
        .append_comment(&item.id, comment("analysis", &first.content))
        .unwrap();
    assert_eq!(retry.id, first.id);
    assert_eq!(
        store
            .append_comment(&item.id, comment("analysis", "覆盖原评论"))
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    let bytes = std::fs::read(active.join("comments.jsonl")).unwrap();
    assert!(bytes.starts_with(&prefix));
    assert_eq!(std::str::from_utf8(&bytes).unwrap().lines().count(), 2);
    assert_eq!(store.item(&item.id).unwrap().revision, item.revision);
    let updated = store
        .update(
            &item.id,
            Update {
                markdown: "# 调整需求".into(),
                ..update(&item)
            },
        )
        .unwrap();
    assert_eq!(std::fs::read(active.join("comments.jsonl")).unwrap(), bytes);
    let reopened = InboxStore::persistent(root.path()).unwrap();
    let comments = reopened.comments(&item.id).unwrap();
    assert_eq!(
        comments.iter().map(|c| &c.id).collect::<Vec<_>>(),
        [&first.id, &second.id]
    );
    assert_eq!(reopened.snapshot().unwrap().comment_counts[&item.id], 2);
    store.delete(&item.id, updated.revision).unwrap();
    assert!(!active.exists());
    assert_eq!(std::fs::read_dir(&deleted).unwrap().count(), 2);
    assert_eq!(
        std::fs::read(deleted.join("comments.jsonl")).unwrap(),
        bytes
    );
    assert_eq!(
        store.comments(&item.id).unwrap_err().status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        store
            .append_comment(&item.id, comment("deleted", "new"))
            .unwrap_err()
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(
        InboxStore::persistent(root.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .items
            .is_empty()
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(config.directory())
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());
}

#[test]
fn comment_commit_failure_is_durable_and_retry_does_not_append_again() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let item = capture(&store, "capture", "需求");
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let lock = config.directory().parent().unwrap().join(".git/index.lock");
    std::fs::write(&lock, "busy").unwrap();
    assert!(
        store
            .append_comment(&item.id, comment("result", "完成"))
            .is_err()
    );
    let saved = store.comments(&item.id).unwrap();
    assert_eq!(saved.len(), 1);
    std::fs::remove_file(lock).unwrap();
    let retry = store
        .append_comment(&item.id, comment("result", "完成"))
        .unwrap();
    assert_eq!(retry.id, saved[0].id);
    assert_eq!(
        InboxStore::persistent(root.path())
            .unwrap()
            .comments(&item.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn concurrent_comment_appends_remain_individual_json_lines() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let item = capture(&store, "capture", "需求");
    std::thread::scope(|scope| {
        for index in 0..8 {
            let store = store.clone();
            let id = item.id.clone();
            scope.spawn(move || {
                store
                    .append_comment(&id, comment(&format!("comment-{index}"), "多行\n评论"))
                    .unwrap()
            });
        }
    });
    assert_eq!(
        InboxStore::persistent(root.path())
            .unwrap()
            .comments(&item.id)
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn legacy_inbox_data_is_ignored_and_invalid_jsonl_is_reported() {
    let root = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigRepository::initialize(root.path()).unwrap();
    std::fs::write(
        config.directory().join("inbox.json"),
        b"discarded legacy data",
    )
    .unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    assert!(store.snapshot().unwrap().items.is_empty());
    let item = capture(&store, "capture", "需求");
    let path = config
        .directory()
        .join("inbox/requirements/active")
        .join(&item.id)
        .join("comments.jsonl");
    std::fs::write(path, b"not a JSON comment\n").unwrap();
    assert!(InboxStore::persistent(root.path()).is_err());
}

#[test]
fn failed_multi_file_update_restores_previous_requirements_and_registry() {
    let root = tempfile::tempdir().unwrap();
    let store = InboxStore::persistent(root.path()).unwrap();
    let mut items = Vec::new();
    for key in ["first", "second"] {
        let item = capture(&store, key, key);
        let mut input = update(&item);
        input.label_ids = vec!["todo".into()];
        items.push(store.update(&item.id, input).unwrap());
    }
    let config = aow_config::ConfigRepository::open(root.path())
        .unwrap()
        .unwrap();
    let inbox = config.directory().join("inbox");
    let first = inbox
        .join("requirements/active")
        .join(&items[0].id)
        .join("requirement.json");
    let second = inbox
        .join("requirements/active")
        .join(&items[1].id)
        .join("requirement.json");
    let original_first = std::fs::read(&first).unwrap();
    let original_second = std::fs::read(&second).unwrap();
    let registry = std::fs::read(inbox.join("labels.json")).unwrap();
    // Force failure after the first requirement has already been updated.
    std::fs::remove_file(&second).unwrap();
    std::fs::create_dir(&second).unwrap();
    let revision = store.snapshot().unwrap().revision;
    assert!(
        store
            .labels(LabelsUpdate {
                expected_revision: revision,
                labels: vec![]
            })
            .is_err()
    );
    assert_eq!(std::fs::read(first).unwrap(), original_first);
    assert_eq!(std::fs::read(inbox.join("labels.json")).unwrap(), registry);
    assert_eq!(store.snapshot().unwrap().revision, revision);
    std::fs::remove_dir(&second).unwrap();
    std::fs::write(second, original_second).unwrap();
    let reopened = InboxStore::persistent(root.path()).unwrap();
    for item in items {
        assert_eq!(reopened.item(&item.id).unwrap().label_ids, ["todo"]);
    }
}

#[tokio::test]
async fn comments_api_only_reads_and_appends_and_emits_inbox_events() {
    let state = crate::AppState::new(Default::default());
    let item = capture(&state.inbox, "capture", "需求");
    let mut changes = state.workspace_events.subscribe();
    let app = crate::build_router(state);
    let path = format!("/api/inbox/items/{}/comments", item.id);
    let input = json!({"request_key":"comment-one","author":{"type":"human","name":"用户"},"content":"**补充**\n\n- 一项"});
    let (status, created) = request(&app, "POST", &path, input.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    let (_, retried) = request(&app, "POST", &path, input).await;
    assert_eq!(created["id"], retried["id"]);
    let (status, comments) = request(&app, "GET", &path, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(comments.as_array().unwrap().len(), 1);
    let (_, snapshot) = request(&app, "GET", "/api/inbox", Value::Null).await;
    assert_eq!(snapshot["comment_counts"][&item.id], 1);
    for method in ["PUT", "DELETE"] {
        assert_eq!(
            request(&app, method, &path, json!({})).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(
                &app,
                method,
                &format!("{path}/{}", created["id"].as_str().unwrap()),
                json!({})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            json!({"request_key":"blank","author":{"type":"ai","name":"Codex"},"content":" "})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

async fn request(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn api_captures_without_project_checks_binding_and_refuses_unbound_execution() {
    let state = crate::AppState::new(Default::default());
    let mut changes = state.workspace_events.subscribe();
    let app = crate::build_router(state);
    let (status, item) = request(
        &app,
        "POST",
        "/api/inbox/items",
        json!({"request_key":"capture", "markdown":"# First line\nbody"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(item.get("title").is_none());
    assert!(item["project_id"].is_null());
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    let path = format!("/api/inbox/items/{}", item["id"].as_str().unwrap());
    let (status, _) = request(
        &app,
        "POST",
        &format!("{path}/execute"),
        json!({"expected_revision":1,"request_key":"run","agent":"codex"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &app,
        "PUT",
        &path,
        json!({"expected_revision":1,"markdown":"same", "project_id":"missing", "label_ids":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(&app, "DELETE", &path, json!({"expected_revision":1})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
