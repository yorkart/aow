use super::{persistence::Persistence, requests::CreationRequest, store::*};
use aow_config::ConfigRepository;
use aow_protocol::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn git(config: &ConfigRepository, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(config.directory())
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn open(state: &Path, config: &ConfigRepository) -> TaskStore {
    TaskStore::new(
        Some(state),
        Some(config.clone()),
        crate::workspace_events::WorkspaceEvents::new(),
    )
    .unwrap()
}
fn capture(store: &TaskStore, id: &str) -> InboxItem {
    store
        .write_inbox(InboxWrite {
            id: id.into(),
            project_id: "project".into(),
            expected_revision: None,
            title: id.into(),
            description: "## Markdown\n\n- Keep this body\n".into(),
        })
        .unwrap()
}
fn task(item: &InboxItem) -> BoardTask {
    serde_json::from_value(json!({"id":"execution", "revision":1,"inbox_id":item.id,"status_id":"todo",
        "title":item.title,"description":item.description,"project_id":item.project_id,"cwd":"/work",
        "agent":"codex","tab_id":null,"pane_id":null,"execution":"ready","error":null,"archived":false,
        "created_at":now(),"updated_at":now(),"history":[]})).unwrap()
}

#[test]
fn individual_requirements_and_statuses_are_configuration_execution_is_local() {
    let state = tempfile::tempdir().unwrap();
    fs::write(
        state.path().join("tasks.json"),
        b"obsolete and deliberately invalid",
    )
    .unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    assert_eq!(store.inbox_page(None, true, 50, None).unwrap().total, 0);
    let first = capture(&store, "first");
    let second = capture(&store, "second");
    let first_path = config
        .directory()
        .join(Persistence::inbox_path("project", "first"));
    let second_path = config
        .directory()
        .join(Persistence::inbox_path("project", "second"));
    let first_bytes = fs::read(&first_path).unwrap();
    let raw: Value = serde_json::from_slice(&first_bytes).unwrap();
    assert!(raw.get("task_ids").is_none());
    assert_eq!(raw["description"], first.description);
    assert!(store.lock().unwrap().memory_inbox.is_empty());

    let mut updated = InboxWrite {
        id: second.id.clone(),
        project_id: "project".into(),
        expected_revision: Some(second.revision),
        title: "Updated".into(),
        description: "new body".into(),
    };
    let updated_item = store.write_inbox(updated.clone()).unwrap();
    assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
    assert!(second_path.exists());
    let changed = git(&config, &["show", "--format=", "--name-only", "HEAD"]);
    assert!(changed.ends_with("tasks/inbox/project/second.json"));
    assert_eq!(changed.lines().count(), 1);
    updated.expected_revision = Some(updated_item.revision);
    let head = git(&config, &["rev-parse", "HEAD"]);
    store.write_inbox(updated).unwrap();
    assert_eq!(git(&config, &["rev-parse", "HEAD"]), head);

    store
        .create_task(task(&first), first.revision, None)
        .unwrap();
    store
        .update_execution("execution", |task| {
            task.status_id = "done".into();
        })
        .unwrap();
    assert_eq!(git(&config, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
    assert_eq!(store.get_inbox("first").unwrap().revision, first.revision);
    let runtime = state
        .path()
        .join("tasks")
        .join(config.selection().config_id)
        .join("tasks.json");
    let runtime_json: Value = serde_json::from_slice(&fs::read(runtime).unwrap()).unwrap();
    assert!(runtime_json.get("inbox").is_none());
    assert!(runtime_json.get("statuses").is_none());
    assert_eq!(runtime_json["tasks"][0]["status_id"], "done");
    assert_eq!(
        store.inbox_page(None, false, 50, None).unwrap().items[0].id,
        "second"
    );
    assert!(store.delete_inbox("first", first.revision).is_err());
    store.delete_inbox("second", updated_item.revision).unwrap();
    assert!(!second_path.exists());
    assert!(first_path.exists());
    assert!(git(&config, &["status", "--porcelain"]).is_empty());
    assert_eq!(
        fs::read(state.path().join("tasks.json")).unwrap(),
        b"obsolete and deliberately invalid"
    );
    let restored = open(state.path(), &config);
    assert_eq!(
        restored.get_inbox("first").unwrap().description,
        first.description
    );
    assert_eq!(
        restored.inbox_page(None, true, 50, None).unwrap().items[0].task_ids,
        ["execution"]
    );
    assert_eq!(restored.get("execution").unwrap().status_id, "done");
}

#[test]
fn configuration_selection_is_pinned_and_runtime_is_namespaced() {
    let state = tempfile::tempdir().unwrap();
    let first = ConfigRepository::initialize(state.path()).unwrap();
    let first_store = open(state.path(), &first);
    let idea = capture(&first_store, "first");
    first_store
        .create_task(task(&idea), idea.revision, None)
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    let second = ConfigRepository::initialize(other.path()).unwrap();
    aow_config::save_selection(state.path(), &second.selection()).unwrap();
    let selected = ConfigRepository::open(state.path()).unwrap().unwrap();
    let second_store = open(state.path(), &selected);
    assert!(second_store.snapshot().unwrap().tasks.is_empty());
    assert_eq!(
        second_store.inbox_page(None, true, 50, None).unwrap().total,
        0
    );
    capture(&first_store, "still-first");
    capture(&second_store, "second");
    assert!(
        !first
            .directory()
            .join(Persistence::inbox_path("project", "second"))
            .exists()
    );
    assert!(
        !second
            .directory()
            .join(Persistence::inbox_path("project", "still-first"))
            .exists()
    );
    assert_eq!(
        open(state.path(), &first).snapshot().unwrap().tasks.len(),
        1
    );
}

#[test]
fn summary_pagination_is_stable_and_filters_before_limiting() {
    let store =
        TaskStore::new(None, None, crate::workspace_events::WorkspaceEvents::new()).unwrap();
    for id in ["a", "b", "c", "d", "e"] {
        capture(&store, id);
    }
    // Make creation times equal to exercise the stable ID tie-breaker.
    {
        let mut data = store.lock().unwrap();
        for item in data.inbox.values_mut() {
            item.created_at = "2026-09-29T00:00:00Z".into();
        }
    }
    let first = store.inbox_page(Some("project"), true, 2, None).unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        ["e", "d"]
    );
    assert_eq!(first.total, 5);
    assert!(
        serde_json::to_value(&first).unwrap()["items"][0]
            .get("description")
            .is_none()
    );
    store.delete_inbox("d", 1).unwrap();
    capture(&store, "new");
    store
        .lock()
        .unwrap()
        .inbox
        .get_mut("new")
        .unwrap()
        .created_at = "2027-01-01T00:00:00Z".into();
    let second = store
        .inbox_page(Some("project"), true, 2, first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        ["c", "b"]
    );
    let last = store
        .inbox_page(Some("project"), true, 2, second.next_cursor.as_deref())
        .unwrap();
    assert_eq!(last.items[0].id, "a");
    assert!(last.next_cursor.is_none());
    assert_eq!(
        store
            .inbox_page(Some("other"), true, 2, None)
            .unwrap()
            .total,
        0
    );
    for limit in [0, 201] {
        assert!(store.inbox_page(None, true, limit, None).is_err());
    }
    assert!(store.inbox_page(None, true, 1, Some("invalid")).is_err());
    let idea = store.get_inbox("new").unwrap();
    store.create_task(task(&idea), idea.revision, None).unwrap();
    let page = store.inbox_page(None, false, 1, None).unwrap();
    assert_eq!(page.total, 4);
    assert_eq!(page.items[0].id, "e");
}

#[test]
fn bodies_are_loaded_on_demand_and_invalid_files_fail_without_rewriting() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    capture(&store, "idea");
    let path = config
        .directory()
        .join(Persistence::inbox_path("project", "idea"));
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    fs::write(&path, b"broken").unwrap();
    assert_eq!(store.inbox_page(None, true, 50, None).unwrap().total, 1);
    assert!(store.get_inbox("idea").is_err());
    let mut no_project = original.clone();
    no_project.as_object_mut().unwrap().remove("project_id");
    let mut wrong_id = original.clone();
    wrong_id["id"] = json!("../outside");
    for bytes in [
        b"broken".to_vec(),
        serde_json::to_vec(&no_project).unwrap(),
        serde_json::to_vec(&wrong_id).unwrap(),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(
            TaskStore::new(
                Some(state.path()),
                Some(config.clone()),
                crate::workspace_events::WorkspaceEvents::new()
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn config_commit_failures_can_retry_capture_edit_and_delete() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    let lock = config.directory().parent().unwrap().join(".git/index.lock");
    let mut input = InboxWrite {
        id: "idea".into(),
        project_id: "project".into(),
        expected_revision: None,
        title: "Idea".into(),
        description: "".into(),
    };
    fs::write(&lock, b"busy").unwrap();
    assert!(store.write_inbox(input.clone()).is_err());
    assert_eq!(store.get_inbox("idea").unwrap().revision, 1);
    fs::remove_file(&lock).unwrap();
    store.write_inbox(input.clone()).unwrap();
    input.expected_revision = Some(1);
    input.title = "Edited".into();
    fs::write(&lock, b"busy").unwrap();
    assert!(store.write_inbox(input.clone()).is_err());
    fs::remove_file(&lock).unwrap();
    assert_eq!(store.write_inbox(input).unwrap().revision, 2);
    fs::write(&lock, b"busy").unwrap();
    assert!(store.delete_inbox("idea", 2).is_err());
    fs::remove_file(&lock).unwrap();
    store.delete_inbox("idea", 2).unwrap();
    assert!(git(&config, &["status", "--porcelain"]).is_empty());
}

#[test]
fn inbox_creation_receipts_survive_commit_failure_restart_and_edits() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    let input = InboxCreate {
        request_key: "capture-request".into(),
        project_id: "project".into(),
        title: "Original".into(),
        description: "Markdown body".into(),
    };
    let lock = config.directory().parent().unwrap().join(".git/index.lock");
    fs::write(&lock, b"busy").unwrap();
    assert!(store.create_inbox(input.clone()).is_err());
    let id = store.inbox_page(None, true, 50, None).unwrap().items[0]
        .id
        .clone();
    assert!(aow_id::is_valid_id(&id));
    assert_ne!(id, input.request_key);
    assert!(
        config
            .directory()
            .join(Persistence::inbox_path("project", &id))
            .is_file()
    );
    assert!(
        !config
            .directory()
            .join(Persistence::inbox_path("project", &input.request_key))
            .exists()
    );
    fs::remove_file(lock).unwrap();
    drop(store);

    let store = open(state.path(), &config);
    let replay = store.create_inbox(input.clone()).unwrap();
    assert_eq!(replay.id, id);
    assert_eq!(replay.revision, 1);
    assert!(git(&config, &["status", "--porcelain"]).is_empty());
    store
        .write_inbox(InboxWrite {
            id: id.clone(),
            project_id: "project".into(),
            expected_revision: Some(1),
            title: "Edited later".into(),
            description: "New body".into(),
        })
        .unwrap();
    drop(store);
    let store = open(state.path(), &config);
    let replay = store.create_inbox(input.clone()).unwrap();
    assert_eq!(replay.id, id);
    assert_eq!(replay.title, "Edited later");
    assert_eq!(replay.revision, 2);
    let mut changed = input.clone();
    changed.title = "Different request".into();
    assert!(store.create_inbox(changed).is_err());
    assert_eq!(store.inbox_page(None, true, 50, None).unwrap().total, 1);
    let mut fresh = input;
    fresh.request_key = "another-capture".into();
    assert_ne!(store.create_inbox(fresh).unwrap().id, id);
}

#[test]
fn status_creation_receipts_survive_commit_failure_and_restart() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    let input = TaskStatusesWrite {
        request_key: "save-statuses".into(),
        expected_revision: store.snapshot().unwrap().status_revision,
        statuses: vec![TaskStatusWrite {
            id: None,
            name: "Draft".into(),
            color: "#123456".into(),
        }],
    };
    let lock = config.directory().parent().unwrap().join(".git/index.lock");
    fs::write(&lock, b"busy").unwrap();
    assert!(store.write_statuses(input.clone()).is_err());
    let saved = store.snapshot().unwrap();
    let id = &saved.statuses[0].id;
    assert!(aow_id::is_valid_id(id));
    fs::remove_file(lock).unwrap();
    drop(store);

    let store = open(state.path(), &config);
    let replay = store.write_statuses(input.clone()).unwrap();
    assert_eq!(replay.statuses, saved.statuses);
    assert_eq!(replay.status_revision, input.expected_revision + 1);
    assert!(git(&config, &["status", "--porcelain"]).is_empty());
    let mut changed = input.clone();
    changed.statuses[0].name = "Different request".into();
    assert!(store.write_statuses(changed).is_err());

    store
        .write_statuses(TaskStatusesWrite {
            request_key: "edit-statuses".into(),
            expected_revision: replay.status_revision,
            statuses: vec![TaskStatusWrite {
                id: Some(id.clone()),
                name: "Accepted".into(),
                color: "#123456".into(),
            }],
        })
        .unwrap();
    assert!(
        store.write_statuses(input).is_err(),
        "an old save cannot overwrite a later revision"
    );
    assert_eq!(store.snapshot().unwrap().statuses[0].name, "Accepted");
}

#[test]
fn concurrent_conversion_is_accepted_once_and_receipt_survives_restart() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let store = open(state.path(), &config);
    let item = capture(&store, "idea");
    let request = CreationRequest::new(
        "convert-request",
        &json!({"inbox_id":item.id,"title":"Execute"}),
    )
    .unwrap();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let store = &store;
                let request = request.clone();
                let barrier = &barrier;
                let mut task = task(&item);
                task.id = aow_id::new_id();
                let revision = item.revision;
                scope.spawn(move || {
                    barrier.wait();
                    store.create_task(task, revision, Some(request)).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|(_, created)| *created).count(), 1);
    assert_eq!(results[0].0.id, results[1].0.id);
    assert_eq!(store.snapshot().unwrap().tasks.len(), 1);
    let id = results[0].0.id.clone();
    store
        .update_execution(&id, |task| {
            task.execution = TaskExecutionPhase::Submitted;
        })
        .unwrap();
    drop(store);

    let store = open(state.path(), &config);
    let replay = store.task_for_request(&request).unwrap().unwrap();
    assert_eq!(replay.id, id);
    assert_eq!(replay.execution, TaskExecutionPhase::Submitted);
    let changed = CreationRequest::new(
        &request.key,
        &json!({"inbox_id":item.id,"title":"Different request"}),
    )
    .unwrap();
    assert!(store.task_for_request(&changed).is_err());
    assert_eq!(store.snapshot().unwrap().tasks.len(), 1);
}
