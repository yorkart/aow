use std::{
    fs,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::Path,
    process::{Command, Output},
};

use aow_automations::{RunEvent, RunSource, RunStatus, Store, Task, runner::initial_run};
use chrono::Utc;
use serde_json::{Value, json};
use tempfile::TempDir;

fn command(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aow-cli"));
    command.env_clear().arg("--state-dir").arg(state);
    command
}

fn success(state: &Path, args: &[&str]) -> Value {
    let output = command(state).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn failure(output: Output, status: i32, code: &str) {
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], code);
    assert!(!error["error"]["message"].as_str().unwrap().is_empty());
}

fn fixture() -> (TempDir, Store, Task) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path().join("state with spaces")).unwrap();
    let task: Task = serde_json::from_value(json!({
        "id":"12345678", "revision":1,
        "created_at":"2026-09-13T09:00:00Z", "updated_at":"2026-09-13T09:00:00Z",
        "name":"Original task", "prompt":"Original prompt", "agent":"codex",
        "project_id":"project-one", "project_name":"Project", "repository_path":"/repo",
        "workspace_mode":"existing", "workspace_path":"/repo", "base_branch":"main",
        "cron":"0 9 * * 1-5", "max_concurrent_runs":1, "enabled":true,
        "launch":{"executable":"/abs/codex", "args":["--color","never"], "environment":{"CODEX_HOME":"/agent/config"}},
        "scheduler_error":null, "deleted":false
    })).unwrap();
    store.save_task(&task).unwrap();
    (directory, store, task)
}

fn finished(store: &Store, task: &Task, id: &str) {
    let mut writer = store
        .create_run(&initial_run(task, id.into(), RunSource::Manual), task)
        .unwrap();
    writer
        .append(&RunEvent::Session {
            session_id: "native-session-id".into(),
            elapsed_ms: 7,
        })
        .unwrap();
    writer
        .append(&RunEvent::Finished {
            at: Utc::now(),
            status: RunStatus::Failed,
            exit_code: Some(9),
            message: Some("Persisted error summary".into()),
            duration_ms: 42,
        })
        .unwrap();
}

#[test]
fn help_and_empty_state_never_initialize_data() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    for args in [
        vec!["--help"],
        vec!["automation", "--help"],
        vec!["automation", "list", "--help"],
        vec!["automation", "get", "--help"],
        vec!["automation", "runs", "--help"],
        vec!["automation", "runs", "list", "--help"],
        vec!["automation", "runs", "get", "--help"],
    ] {
        let output = command(&missing).args(args).output().unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        for text in [
            "Examples",
            "AOW_STATE_DIR",
            "filesystem permissions",
            "Exit codes",
        ] {
            assert!(help.contains(text), "{help}");
        }
    }
    assert!(
        command(&missing)
            .arg("--version")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        success(&missing, &["automation", "list"]),
        json!({"items":[]})
    );
    failure(
        command(&missing)
            .args(["automation", "get", "12345678"])
            .output()
            .unwrap(),
        3,
        "not_found",
    );
    failure(
        command(&missing)
            .args(["automation", "runs", "list", "12345678"])
            .output()
            .unwrap(),
        3,
        "not_found",
    );
    assert!(!missing.exists());
}

#[test]
fn arguments_are_validated_before_reading_state() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    for args in [
        vec!["automation", "get", "../escape"],
        vec!["automation", "runs", "list", "12345678", "--limit", "0"],
        vec!["automation", "runs", "list", "12345678", "--limit", "501"],
        vec![
            "automation",
            "runs",
            "list",
            "12345678",
            "--before",
            "../escape",
        ],
        vec!["automation", "run", "12345678"],
        vec!["automation", "get"],
    ] {
        failure(
            command(&missing).args(args).output().unwrap(),
            2,
            "invalid_arguments",
        );
    }
    assert!(!missing.exists());
}

#[test]
fn configuration_snapshots_pagination_and_deleted_history() {
    let (_temp, store, mut task) = fixture();
    for id in [
        "20260913T090000000Z_0001",
        "20260913T090000000Z_0002",
        "20260913T090000000Z_0003",
    ] {
        finished(&store, &task, id);
    }
    task.input.prompt = "Updated prompt".into();
    task.revision = 2;
    store.save_task(&task).unwrap();
    let detail = success(&store.state_dir, &["automation", "get", &task.id]);
    assert_eq!(detail["prompt"], "Updated prompt");
    assert_eq!(detail["cleanup_worktree"], true);
    assert_eq!(detail["launch"]["executable"], "/abs/codex");
    assert_eq!(detail["is_running"], false);
    let first = success(
        &store.state_dir,
        &["automation", "runs", "list", &task.id, "--limit", "2"],
    );
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["items"][0]["id"], "20260913T090000000Z_0003");
    assert_eq!(
        first["items"][0]["configuration"]["prompt"],
        "Original prompt"
    );
    assert_eq!(first["items"][0]["task_revision"], 1);
    assert_eq!(first["items"][0]["status"], "failed");
    let cursor = first["next_cursor"].as_str().unwrap();
    // A cursor remains usable after that record has been cleaned up.
    fs::remove_dir_all(store.run_path(&task.id, cursor).unwrap()).unwrap();
    let second = success(
        &store.state_dir,
        &[
            "automation",
            "runs",
            "list",
            &task.id,
            "--limit",
            "2",
            "--before",
            cursor,
        ],
    );
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());
    assert_eq!(second["items"][0]["id"], "20260913T090000000Z_0001");
    let run = success(
        &store.state_dir,
        &[
            "automation",
            "runs",
            "get",
            &task.id,
            "20260913T090000000Z_0003",
        ],
    );
    assert_eq!(run["message"], "Persisted error summary");
    assert_eq!(run["session_id"], "native-session-id");
    assert_eq!(run["exit_code"], 9);
    assert_eq!(
        run["stdout_path"],
        store
            .run_path(&task.id, "20260913T090000000Z_0003")
            .unwrap()
            .join("stdio")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        success(
            &store.state_dir,
            &["automation", "list", "--project-id", "other"]
        )["items"],
        json!([])
    );
    task.deleted = true;
    store.save_task(&task).unwrap();
    assert_eq!(
        success(&store.state_dir, &["automation", "list"])["items"],
        json!([])
    );
    assert_eq!(
        success(
            &store.state_dir,
            &["automation", "list", "--include-deleted"]
        )["items"][0]["deleted"],
        true
    );
    assert_eq!(
        success(&store.state_dir, &["automation", "get", &task.id])["deleted"],
        true
    );
    assert_eq!(
        success(
            &store.state_dir,
            &[
                "automation",
                "runs",
                "get",
                &task.id,
                "20260913T090000000Z_0003"
            ]
        )["status"],
        "failed"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct StateEntry {
    path: std::path::PathBuf,
    mode: u32,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    contents: Vec<u8>,
}

fn snapshot(path: &Path) -> Vec<StateEntry> {
    let metadata = fs::metadata(path).unwrap();
    let mut result = vec![StateEntry {
        path: path.to_owned(),
        mode: metadata.mode(),
        inode: metadata.ino(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        contents: if metadata.is_file() {
            fs::read(path).unwrap_or_default()
        } else {
            Vec::new()
        },
    }];
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            result.extend(snapshot(&entry));
        }
    }
    result
}

fn permissions(path: &Path, readonly: bool) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            permissions(&path, readonly);
        }
        let mode = if readonly {
            if path.is_dir() { 0o555 } else { 0o444 }
        } else {
            0o700
        };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if readonly { 0o555 } else { 0o700 }),
    )
    .unwrap();
}

#[test]
fn queries_work_on_readonly_state_and_do_not_read_output_files() {
    let (_temp, store, task) = fixture();
    let run_id = "20260913T090000000Z_0001";
    finished(&store, &task, run_id);
    fs::write(
        store.root.join("runs").join(&task.id).join("slot-1.lock"),
        "",
    )
    .unwrap();
    permissions(&store.state_dir, true);
    for name in ["stdio", "stderr"] {
        fs::set_permissions(
            store.run_path(&task.id, run_id).unwrap().join(name),
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();
    }
    let before = snapshot(&store.state_dir);
    success(&store.state_dir, &["automation", "list"]);
    success(&store.state_dir, &["automation", "get", &task.id]);
    success(&store.state_dir, &["automation", "runs", "list", &task.id]);
    success(
        &store.state_dir,
        &["automation", "runs", "get", &task.id, run_id],
    );
    let after = snapshot(&store.state_dir);
    permissions(&store.state_dir, false);
    assert_eq!(before, after);
}

#[test]
fn running_interrupted_and_corrupt_records_have_distinct_results() {
    let (_temp, store, task) = fixture();
    let run_id = "20260913T090000000Z_0001";
    let writer = store
        .create_run(&initial_run(&task, run_id.into(), RunSource::Manual), &task)
        .unwrap();
    assert_eq!(
        success(&store.state_dir, &["automation", "get", &task.id])["is_running"],
        true
    );
    assert_eq!(
        success(
            &store.state_dir,
            &["automation", "runs", "get", &task.id, run_id]
        )["status"],
        "preparing"
    );
    drop(writer);
    let path = store
        .run_path(&task.id, run_id)
        .unwrap()
        .join("events.jsonl");
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    // Another reader's shared lock must not be mistaken for an active runner.
    let reader = fs::File::open(&path).unwrap();
    assert_eq!(
        unsafe { libc::flock(reader.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) },
        0
    );
    let interrupted = success(
        &store.state_dir,
        &["automation", "runs", "get", &task.id, run_id],
    );
    assert_eq!(interrupted["status"], "interrupted");
    assert!(interrupted["finished_at"].is_null());
    drop(reader);
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    failure(
        command(&store.state_dir)
            .args(["automation", "runs", "get", &task.id, run_id])
            .output()
            .unwrap(),
        5,
        "invalid_data",
    );
    failure(
        command(&store.state_dir)
            .args(["automation", "runs", "get", &task.id, "missing-run"])
            .output()
            .unwrap(),
        3,
        "not_found",
    );
}

#[test]
fn external_configuration_keeps_local_run_history_and_legacy_reads_work() {
    let (temp, store, task) = fixture();
    let run_id = "20260913T090000000Z_0001";
    finished(&store, &task, run_id);
    let clone = temp.path().join("external config clone");
    let result = Command::new("git")
        .arg("clone")
        .arg(store.config_dir.parent().unwrap())
        .arg(&clone)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let selected = clone.join(store.config_dir.file_name().unwrap());
    aow_config::save_selection(
        &store.state_dir,
        &aow_config::ConfigSelection {
            config_repo: clone,
            config_id: store
                .config_dir
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        },
    )
    .unwrap();
    let switched = Store::open(store.state_dir.clone()).unwrap();
    let mut changed = task.clone();
    changed.input.name = "From external configuration".into();
    switched.save_task(&changed).unwrap();
    assert_eq!(
        success(&store.state_dir, &["automation", "get", &task.id])["name"],
        changed.input.name
    );
    assert_eq!(
        success(
            &store.state_dir,
            &["automation", "runs", "get", &task.id, run_id]
        )["status"],
        "failed"
    );
    assert_eq!(
        store.get_task(&task.id).unwrap().input.name,
        task.input.name
    );
    assert_eq!(switched.root, store.root);
    assert!(!selected.join("automations/runs").exists());

    let legacy = temp.path().join("legacy-state");
    fs::create_dir_all(legacy.join("automations/tasks")).unwrap();
    fs::copy(
        store.task_path(&task.id).unwrap(),
        legacy
            .join("automations/tasks")
            .join(format!("{}.json", task.id)),
    )
    .unwrap();
    let before = snapshot(&legacy);
    assert_eq!(
        success(&legacy, &["automation", "get", &task.id])["name"],
        task.input.name
    );
    assert_eq!(snapshot(&legacy), before);
    assert!(!legacy.join("__current__").exists());
    assert!(!legacy.join("config.toml").exists());
}

#[test]
fn permission_errors_are_not_empty_lists() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (_temp, store, _task) = fixture();
    fs::set_permissions(
        store.config_dir.join("automations/tasks"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let output = command(&store.state_dir)
        .args(["automation", "list"])
        .output()
        .unwrap();
    fs::set_permissions(
        store.config_dir.join("automations/tasks"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    failure(output, 4, "permission_denied");
}

#[test]
fn state_directory_precedence_matches_server() {
    let (temp, store, _task) = fixture();
    let invoke = |explicit: bool, aow: bool, xdg: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_aow-cli"));
        cmd.env_clear().env("HOME", temp.path().join("home"));
        if aow {
            cmd.env("AOW_STATE_DIR", &store.state_dir);
        }
        if xdg {
            cmd.env("XDG_STATE_HOME", temp.path().join("xdg"));
        }
        if explicit {
            cmd.arg("--state-dir").arg(temp.path().join("explicit"));
        }
        cmd.args(["automation", "list", "--compact"])
            .output()
            .unwrap()
    };
    for (explicit, aow, xdg, count) in [
        (true, true, true, 0),
        (false, true, true, 1),
        (false, false, true, 0),
        (false, false, false, 0),
    ] {
        let output = invoke(explicit, aow, xdg);
        assert!(output.status.success(), "{output:?}");
        let data: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(data["items"].as_array().unwrap().len(), count);
    }
    for path in ["explicit", "home", "xdg"] {
        assert!(!temp.path().join(path).exists());
    }
}
