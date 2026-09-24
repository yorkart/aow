use std::{
    collections::BTreeMap, fs, io::Write, os::unix::fs::PermissionsExt, path::Path, process::Stdio,
    time::Duration,
};

use aow_automations::{
    AgentKind, AgentLaunch, Run, RunEvent, RunSource, RunStatus, Scheduler, Store, Task, TaskInput,
    WorkspaceMode, runner,
    scheduler::Platform,
    store::{MAX_RUN_OUTPUT_BYTES, new_run_id},
};
use chrono::Utc;
use tempfile::TempDir;
use tokio::process::Command;

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn fixture(script: &str) -> (TempDir, Store, Task) {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repo with spaces");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-b", "main"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Automation Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let agent = directory.path().join("fake agent");
    executable(&agent, script);
    let store = Store::new(directory.path().join("state with spaces")).unwrap();
    let task = Task {
        id: "task-one".into(),
        revision: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        input: TaskInput {
            kind: Default::default(),
            prompt_bindings: Vec::new(),
            name: "Test task".into(),
            prompt: "original prompt".into(),
            agent: AgentKind::Codex,
            project_id: "project".into(),
            workspace_mode: WorkspaceMode::Existing,
            workspace_path: repository.clone(),
            cleanup_worktree: false,
            base_branch: "main".into(),
            cron: "0 9 * * 1-5".into(),
            interval_seconds: None,
            max_concurrent_runs: 3,
            enabled: true,
            yolo: true,
            precheck_command: String::new(),
            precheck_timeout_seconds: 1,
            failure_notification: None,
        },
        project_name: "Project".into(),
        repository_path: repository,
        launch: AgentLaunch {
            executable: agent,
            args: Vec::new(),
            environment: BTreeMap::new(),
        },
        scheduler_error: None,
        deleted: false,
    };
    store.save_task(&task).unwrap();
    (directory, store, task)
}

const CODEX: &str = r#"
printf 'session id: session-%s\n' "$$" >&2
cat > /dev/null
printf '{"type":"item.completed","text":"DO_NOT_STORE_AGENT_OUTPUT"}\n'
head -c 200000 /dev/zero
"#;

#[tokio::test]
async fn global_path_is_reloaded_between_runs_and_shared_by_precheck_and_agent_children() {
    let (directory, store, mut task) = fixture(
        "printf 'session id: environment-session\\n' >&2\ncat >/dev/null\n/bin/sh -c python3 > agent-python",
    );
    let settings_path = store
        .config_dir
        .join(aow_agents::environment::SETTINGS_FILE);
    let next_settings = directory.path().join("next settings.json");
    for (version, destination) in [("first", &settings_path), ("second", &next_settings)] {
        let bin = directory.path().join(format!("python {version} bin"));
        fs::create_dir(&bin).unwrap();
        executable(&bin.join("python3"), &format!("printf '{version}'"));
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        fs::write(
            destination,
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "execution_path": paths,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    // Changing global Settings during precheck must not change this run's snapshot.
    task.input.precheck_command = format!(
        "python3 > precheck-python && cp '{}' '{}'",
        next_settings.display(),
        settings_path.display(),
    );
    task.launch
        .environment
        .insert("PATH".into(), "/obsolete/task/path".into());
    store.save_task(&task).unwrap();
    let task_before = fs::read(store.task_path(&task.id).unwrap()).unwrap();
    for version in ["first", "second"] {
        let id = new_run_id();
        let status = runner::run(&store, &task.id, Some(id.clone()), RunSource::Scheduled)
            .await
            .unwrap();
        let run = store.read_run(&task.id, &id).unwrap().unwrap();
        assert_eq!(status, RunStatus::Completed, "{:?}", run.message);
        for output in ["precheck-python", "agent-python"] {
            assert_eq!(
                fs::read_to_string(task.repository_path.join(output)).unwrap(),
                version
            );
        }
    }
    assert_eq!(
        fs::read(store.task_path(&task.id).unwrap()).unwrap(),
        task_before
    );
}

#[tokio::test]
async fn registered_agent_environment_is_reloaded_between_runs_and_snapshotted_per_run() {
    let (directory, store, mut task) = fixture(
        "printf 'session id: environment-session\n' >&2\ncat >/dev/null\nprintf '%s' \"$DYNAMIC_AGENT_ENV\" > agent-environment",
    );
    let registry_path = store.config_dir.join(aow_agents::environment::AGENTS_FILE);
    let next_registry = directory.path().join("next agents.json");
    let registry = |value: &str| {
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "items": [{
                "id": "codex",
                "display_name": "Codex",
                "command": "codex",
                "env": {"DYNAMIC_AGENT_ENV": value, "PATH": "/ignored/agent/path"}
            }]
        }))
        .unwrap()
    };
    fs::write(&registry_path, registry("first")).unwrap();
    fs::write(&next_registry, registry("second")).unwrap();
    task.input.precheck_command = format!(
        "printf '%s' \"$DYNAMIC_AGENT_ENV\" > precheck-environment && cp '{}' '{}'",
        next_registry.display(),
        registry_path.display(),
    );
    task.launch
        .environment
        .insert("DYNAMIC_AGENT_ENV".into(), "task-snapshot".into());
    store.save_task(&task).unwrap();
    let task_before = fs::read(store.task_path(&task.id).unwrap()).unwrap();

    for expected in ["first", "second"] {
        let id = new_run_id();
        let status = runner::run(&store, &task.id, Some(id.clone()), RunSource::Scheduled)
            .await
            .unwrap();
        let run = store.read_run(&task.id, &id).unwrap().unwrap();
        assert_eq!(status, RunStatus::Completed, "{:?}", run.message);
        assert_eq!(
            fs::read_to_string(task.repository_path.join("precheck-environment")).unwrap(),
            "task-snapshot"
        );
        assert_eq!(
            fs::read_to_string(task.repository_path.join("agent-environment")).unwrap(),
            expected
        );
    }
    assert_eq!(
        fs::read(store.task_path(&task.id).unwrap()).unwrap(),
        task_before
    );
}

#[tokio::test]
async fn invalid_global_path_fails_before_precheck_or_agent_instead_of_using_legacy_path() {
    let (_directory, store, mut task) = fixture("touch agent-started");
    task.input.precheck_command = "touch precheck-started".into();
    task.launch
        .environment
        .insert("PATH".into(), std::env::var("PATH").unwrap());
    fs::write(
        store
            .config_dir
            .join(aow_agents::environment::SETTINGS_FILE),
        br#"{"version":1,"execution_path":["relative/bin"]}"#,
    )
    .unwrap();
    let run = execute(&store, &task).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.message.unwrap().contains("PATH"));
    assert!(run.agent_pid.is_none());
    assert!(!task.repository_path.join("precheck-started").exists());
    assert!(!task.repository_path.join("agent-started").exists());
}

fn command_arguments(task: &Task, session_id: Option<&str>) -> Vec<String> {
    aow_automations::agent::command(task, &BTreeMap::new(), &task.repository_path, session_id)
        .unwrap()
        .as_std()
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn yolo_uses_the_native_flag_for_each_agent_and_defaults_to_enabled() {
    let (_directory, _store, mut task) = fixture(CODEX);
    for agent in [AgentKind::Codex, AgentKind::TraeCli] {
        task.input.agent = agent;
        task.input.yolo = true;
        let arguments = command_arguments(&task, None);
        assert!(arguments.contains(&"--yolo".into()));
        assert!(!arguments.contains(&"--sandbox".into()));
        assert!(arguments.windows(2).any(|pair| pair == ["exec", "--color"]));
        assert!(!arguments.contains(&"--json".into()));

        task.input.yolo = false;
        let arguments = command_arguments(&task, None);
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--ask-for-approval", "never"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--sandbox", "workspace-write"])
        );
        assert!(!arguments.contains(&"--yolo".into()));
    }

    task.input.agent = AgentKind::Claude;
    task.input.yolo = true;
    assert!(
        command_arguments(&task, Some("session"))
            .contains(&"--dangerously-skip-permissions".into())
    );
    task.input.yolo = false;
    assert!(
        !command_arguments(&task, Some("session"))
            .contains(&"--dangerously-skip-permissions".into())
    );

    let defaults: TaskInput = serde_json::from_value(serde_json::json!({
        "name": "Default settings", "prompt": "Do work", "agent": "codex",
        "project_id": "project", "workspace_mode": "existing",
        "workspace_path": "/workspace", "cron": "0 9 * * *", "max_concurrent_runs": 3, "enabled": true
    }))
    .unwrap();
    assert!(defaults.yolo);
    assert!(defaults.cleanup_worktree);
}

async fn execute(store: &Store, task: &Task) -> Run {
    store.save_task(task).unwrap();
    let id = new_run_id();
    runner::run(store, &task.id, Some(id.clone()), RunSource::Manual)
        .await
        .unwrap();
    store.read_run(&task.id, &id).unwrap().unwrap()
}

#[tokio::test]
async fn captures_codex_and_traecli_sessions_and_persists_raw_output() {
    let (_directory, store, mut task) = fixture(CODEX);
    let mut ids = Vec::new();
    for kind in [AgentKind::Codex, AgentKind::TraeCli] {
        task.input.agent = kind;
        let run = execute(&store, &task).await;
        assert_eq!(run.status, RunStatus::Completed, "{:?}", run.message);
        assert!(run.session_id.as_ref().unwrap().starts_with("session-"));
        let command = run.agent_command.as_ref().unwrap();
        assert_eq!(
            command.first().unwrap(),
            &task.launch.executable.to_string_lossy()
        );
        assert!(!command.contains(&"--json".into()));
        assert!(command.contains(&"--yolo".into()));
        ids.push(run.session_id.unwrap());
        let run_directory = store.run_path(&task.id, &run.id).unwrap();
        let journal = fs::read_to_string(run_directory.join("events.jsonl")).unwrap();
        let output = fs::read_to_string(run_directory.join("stdio")).unwrap();
        let stderr = fs::read_to_string(run_directory.join("stderr")).unwrap();
        assert!(output.contains("DO_NOT_STORE_AGENT_OUTPUT"));
        assert!(stderr.contains("session-"));
        assert!(journal.len() < 5000);
        assert!(journal.contains("original prompt"));
        assert!(journal.contains(r#""command":["#));
        assert!(run_directory.join("stderr").is_file());
        assert!(!store.is_running(&task.id).unwrap());
    }
    assert_ne!(ids[0], ids[1]);
}

#[tokio::test]
async fn output_files_are_bounded_and_truncation_is_recorded() {
    let (_directory, store, task) = fixture(
        r#"
printf 'session id: large-output-session\n'
cat >/dev/null
head -c 33554433 /dev/zero
head -c 33554433 /dev/zero >&2
"#,
    );
    let run = execute(&store, &task).await;
    assert_eq!(run.status, RunStatus::Completed, "{:?}", run.message);
    let directory = store.run_path(&task.id, &run.id).unwrap();
    assert_eq!(
        fs::metadata(directory.join("stdio")).unwrap().len(),
        MAX_RUN_OUTPUT_BYTES
    );
    assert_eq!(
        fs::metadata(directory.join("stderr")).unwrap().len(),
        MAX_RUN_OUTPUT_BYTES
    );
    let events = fs::read_to_string(directory.join("events.jsonl")).unwrap();
    assert!(events.contains(r#""type":"output_truncated","output":"stdio""#));
    assert!(events.contains(r#""type":"output_truncated","output":"stderr""#));
}

#[tokio::test]
async fn claude_receives_a_fresh_uuid_and_prompt_on_stdin() {
    let (directory, store, mut task) = fixture(
        r#"
printf '%s\n' "$@" > "$TEST_ARGS"
cat > "$TEST_PROMPT"
"#,
    );
    task.input.agent = AgentKind::Claude;
    let args = directory.path().join("args");
    let prompt = directory.path().join("prompt");
    task.launch
        .environment
        .insert("TEST_ARGS".into(), args.to_str().unwrap().into());
    task.launch
        .environment
        .insert("TEST_PROMPT".into(), prompt.to_str().unwrap().into());
    let first = execute(&store, &task).await;
    let second = execute(&store, &task).await;
    assert_eq!(second.status, RunStatus::Completed);
    let command = second.agent_command.as_ref().unwrap();
    assert_eq!(
        command.first().unwrap(),
        &task.launch.executable.to_string_lossy()
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--session-id", second.session_id.as_ref().unwrap()])
    );
    assert_ne!(first.session_id, second.session_id);
    let id = second.session_id.unwrap();
    uuid::Uuid::parse_str(&id).unwrap();
    let arguments = fs::read_to_string(args).unwrap();
    assert!(arguments.contains(&format!("--session-id\n{id}\n")));
    assert!(arguments.contains("--print\n"));
    assert!(!arguments.contains("resume"));
    assert_eq!(fs::read_to_string(prompt).unwrap(), "original prompt");
}

#[tokio::test]
async fn worktrees_are_always_cleaned_and_branch_switches_reject_dirty_workspaces() {
    let (_directory, store, mut task) = fixture(CODEX);
    task.input.workspace_mode = WorkspaceMode::NewWorktree;
    let first = execute(&store, &task).await;
    let second = execute(&store, &task).await;
    assert_eq!(first.status, RunStatus::Completed, "{:?}", first.message);
    assert_ne!(first.workspace_path, second.workspace_path);
    assert_ne!(first.branch, second.branch);
    assert!(!first.workspace_path.as_ref().unwrap().exists());
    assert!(!second.workspace_path.as_ref().unwrap().exists());
    assert_eq!(
        git(&task.repository_path, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    assert_eq!(
        git(&task.repository_path, &["branch", "--show-current"]),
        "main"
    );
    // Runtime cleanup is mandatory even for old or direct API task payloads
    // that still carry the legacy false flag.
    task.input.cleanup_worktree = false;
    let forced_cleanup = execute(&store, &task).await;
    assert_eq!(forced_cleanup.status, RunStatus::Completed);
    assert!(!forced_cleanup.workspace_path.as_ref().unwrap().exists());
    task.input.workspace_mode = WorkspaceMode::NewBranch;
    fs::write(task.repository_path.join("dirty.txt"), "keep me").unwrap();
    let dirty = execute(&store, &task).await;
    assert_eq!(dirty.status, RunStatus::Failed);
    assert!(dirty.session_id.is_none());
    assert_eq!(
        git(&task.repository_path, &["branch", "--show-current"]),
        "main"
    );
    assert_eq!(
        fs::read_to_string(task.repository_path.join("dirty.txt")).unwrap(),
        "keep me"
    );
    fs::remove_file(task.repository_path.join("dirty.txt")).unwrap();
    let branch = execute(&store, &task).await;
    assert_eq!(branch.status, RunStatus::Completed);
    assert_eq!(
        git(&task.repository_path, &["branch", "--show-current"]),
        branch.branch.unwrap()
    );
}

#[tokio::test]
async fn temporary_workspace_runs_outside_the_repository_and_is_removed_afterwards() {
    let (_directory, store, mut task) = fixture(
        "printf 'session id: temporary-session\n' >&2\ncat >/dev/null\nprintf '%s' \"$PWD\" > executed-cwd",
    );
    task.input.workspace_mode = WorkspaceMode::Temporary;
    task.input.base_branch.clear();
    task.input.precheck_command = "test ! -d .git".into();

    let run = execute(&store, &task).await;

    assert_eq!(run.status, RunStatus::Completed, "{:?}", run.message);
    assert_eq!(run.branch, None);
    assert_eq!(run.session_id.as_deref(), Some("temporary-session"));
    let path = run.workspace_path.unwrap();
    assert!(path.starts_with(std::env::temp_dir()));
    assert_ne!(path, task.repository_path);
    assert!(
        !path.exists(),
        "temporary workspace must be removed after the run"
    );
    assert!(!task.repository_path.join("executed-cwd").exists());
}

#[tokio::test]
async fn cleanup_removes_dirty_worktrees_after_failed_and_skipped_runs() {
    let (_directory, store, mut task) = fixture(
        "printf 'session id: failed-session\n'\ncat >/dev/null\ntouch generated.txt\nexit 17",
    );
    task.input.workspace_mode = WorkspaceMode::NewWorktree;
    task.input.cleanup_worktree = true;
    let failed = execute(&store, &task).await;
    assert_eq!(failed.status, RunStatus::Failed);
    assert_eq!(failed.exit_code, Some(17));
    assert!(!failed.workspace_path.unwrap().exists());

    task.input.precheck_command = "exit 1".into();
    let skipped = execute(&store, &task).await;
    assert_eq!(skipped.status, RunStatus::Skipped);
    assert!(skipped.agent_pid.is_none());
    assert!(!skipped.workspace_path.unwrap().exists());
}

#[tokio::test]
async fn missing_session_nonzero_exit_precheck_and_paused_tasks_have_distinct_results() {
    let (_directory, store, mut task) = fixture("cat >/dev/null");
    let missing = execute(&store, &task).await;
    assert_eq!(missing.status, RunStatus::Failed);
    assert!(missing.message.unwrap().contains("会话 ID"));
    executable(
        &task.launch.executable,
        "cat >/dev/null\nprintf 'authentication failed' >&2\nexit 17",
    );
    let failed = execute(&store, &task).await;
    assert_eq!(failed.exit_code, Some(17));
    assert_eq!(failed.message.as_deref(), Some("authentication failed"));
    task.input.precheck_command = "exit 1".into();
    let skipped = execute(&store, &task).await;
    assert_eq!(skipped.status, RunStatus::Skipped);
    assert!(skipped.agent_pid.is_none());
    task.input.precheck_command = "sleep 5".into();
    let timeout = execute(&store, &task).await;
    assert_eq!(timeout.status, RunStatus::Failed);
    assert!(timeout.message.unwrap().contains("超时"));
    task.input.enabled = false;
    store.save_task(&task).unwrap();
    let id = new_run_id();
    assert_eq!(
        runner::run(&store, &task.id, Some(id.clone()), RunSource::Scheduled)
            .await
            .unwrap(),
        RunStatus::Skipped
    );
    assert!(
        store
            .read_run(&task.id, &id)
            .unwrap()
            .unwrap()
            .agent_pid
            .is_none()
    );
}

#[tokio::test]
async fn live_session_is_visible_before_exit_and_survives_configuration_changes() {
    let (directory, store, mut task) = fixture(
        r#"
printf 'session id: early-session\n'
cat > /dev/null
while [ ! -f "$TEST_RELEASE" ]; do sleep 0.02; done
"#,
    );
    task.input.max_concurrent_runs = 1;
    let release = directory.path().join("release");
    task.launch
        .environment
        .insert("TEST_RELEASE".into(), release.to_str().unwrap().into());
    store.save_task(&task).unwrap();
    let id = new_run_id();
    let mut process = Command::new(env!("CARGO_BIN_EXE_aow-automation-runner"))
        .args([
            "run",
            "--state-dir",
            store.state_dir.to_str().unwrap(),
            "--task-id",
            &task.id,
            "--run-id",
            &id,
        ])
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Some(run)) = store.read_run(&task.id, &id)
                && run.session_id.is_some()
            {
                assert_eq!(run.status, RunStatus::Running);
                assert!(store.is_running(&task.id).unwrap());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(process.try_wait().unwrap().is_none());
    task.input.prompt = "edited prompt".into();
    task.revision = 2;
    store.save_task(&task).unwrap();
    let duplicate_id = new_run_id();
    assert_eq!(
        runner::run(
            &store,
            &task.id,
            Some(duplicate_id.clone()),
            RunSource::Manual
        )
        .await
        .unwrap(),
        RunStatus::Skipped
    );
    assert!(!store.run_path(&task.id, &duplicate_id).unwrap().exists());
    assert!(store.is_running(&task.id).unwrap());
    fs::write(release, "done").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), process.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let fresh_reader = Store::new(store.state_dir.clone()).unwrap();
    let run = fresh_reader.read_run(&task.id, &id).unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.task_revision, 1);
    let raw =
        fs::read_to_string(store.run_path(&task.id, &id).unwrap().join("events.jsonl")).unwrap();
    assert!(raw.contains("original prompt"));
    assert!(!raw.contains("edited prompt"));
    assert!(!fresh_reader.is_running(&task.id).unwrap());
}

#[tokio::test]
async fn sigterm_leaves_an_interrupted_record_with_session_id() {
    let (_directory, store, mut task) =
        fixture("printf 'session id: stopped-session\\n'\ncat >/dev/null\nsleep 60");
    task.input.workspace_mode = WorkspaceMode::NewWorktree;
    task.input.cleanup_worktree = true;
    store.save_task(&task).unwrap();
    let id = new_run_id();
    let mut process = Command::new(env!("CARGO_BIN_EXE_aow-automation-runner"))
        .args([
            "run",
            "--state-dir",
            store.state_dir.to_str().unwrap(),
            "--task-id",
            &task.id,
            "--run-id",
            &id,
        ])
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Some(run)) = store.read_run(&task.id, &id)
                && run.session_id.is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        unsafe { libc::kill(process.id().unwrap() as i32, libc::SIGTERM) },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    assert!(
        !tokio::time::timeout(Duration::from_secs(10), process.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let run = store.read_run(&task.id, &id).unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Interrupted);
    assert_eq!(run.session_id.as_deref(), Some("stopped-session"));
    assert!(!run.workspace_path.unwrap().exists());
}

#[test]
fn journals_have_single_owners_tolerate_truncated_tails_and_page_by_timestamp() {
    let (_directory, store, task) = fixture(CODEX);
    let id = new_run_id();
    let run = runner::initial_run(&task, id.clone(), RunSource::Manual);
    let mut writer = store.create_run(&run, &task).unwrap();
    assert!(store.create_run(&run, &task).is_err());
    assert_eq!(
        store.read_run(&task.id, &id).unwrap().unwrap().status,
        RunStatus::Preparing
    );
    writer
        .append(&RunEvent::Session {
            session_id: "saved-session".into(),
            elapsed_ms: 42,
        })
        .unwrap();
    drop(writer);
    fs::OpenOptions::new()
        .append(true)
        .open(store.run_path(&task.id, &id).unwrap().join("events.jsonl"))
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    let interrupted = store.read_run(&task.id, &id).unwrap().unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.session_id.as_deref(), Some("saved-session"));
    let legacy_id = new_run_id();
    let legacy_run = runner::initial_run(&task, legacy_id.clone(), RunSource::Manual);
    let legacy_path = store.run_path(&task.id, &legacy_id).unwrap();
    fs::create_dir(&legacy_path).unwrap();
    fs::write(
        legacy_path.join("events.jsonl"),
        format!(
            "{{\"type\":\"started\",\"run\":{},\"configuration\":{{\"prevent_overlap\":true}}}}\n",
            serde_json::to_string(&legacy_run).unwrap()
        ),
    )
    .unwrap();
    assert!(store.read_run(&task.id, &legacy_id).is_err());
    assert!(store.run_path("../escape", &id).is_err());
    assert!(store.run_path(&task.id, "../../escape").is_err());
    for id in ["20260910T010000000Z_b", "20260911T010000000Z_c"] {
        let run = runner::initial_run(&task, id.into(), RunSource::Manual);
        drop(store.create_run(&run, &task).unwrap());
    }
    let page = store
        .runs(&task.id, Some("20260910T010000000Z_c"), 1)
        .unwrap();
    assert_eq!(page[0].id, "20260910T010000000Z_b");
}

#[test]
fn run_history_retention_is_per_task_and_does_not_remove_active_runs() {
    let (_directory, store, task) = fixture(CODEX);
    let mut second_task = task.clone();
    second_task.id = "task-two".into();
    store.save_task(&second_task).unwrap();

    for task in [&task, &second_task] {
        for index in 0..=200 {
            let id = format!("20260910T010000000Z_{index:04}");
            let run = runner::initial_run(task, id, RunSource::Manual);
            drop(store.create_run(&run, task).unwrap());
        }
    }

    assert_eq!(store.prune_run_history(200).unwrap(), 2);
    for task in [&task, &second_task] {
        assert_eq!(store.runs(&task.id, None, 500).unwrap().len(), 200);
        assert!(
            !store
                .run_path(&task.id, "20260910T010000000Z_0000")
                .unwrap()
                .exists()
        );
        assert!(
            store
                .run_path(&task.id, "20260910T010000000Z_0200")
                .unwrap()
                .exists()
        );
    }

    let active_id = "20260910T000000000Z_active";
    let active = runner::initial_run(&task, active_id.into(), RunSource::Manual);
    let active_writer = store.create_run(&active, &task).unwrap();
    assert_eq!(store.prune_run_history(200).unwrap(), 0);
    assert!(store.run_path(&task.id, active_id).unwrap().exists());
    drop(active_writer);
}

#[tokio::test]
async fn native_timer_rendering_sync_and_dispatch_use_only_detached_runner_commands() {
    let (directory, store, mut task) = fixture(CODEX);
    let manager = directory.path().join("fake-manager");
    executable(&manager, "exit 0");
    let dispatcher = directory.path().join("fake-dispatcher");
    executable(&dispatcher, "exit 1");
    let mut scheduler = Scheduler {
        platform: Platform::Systemd,
        runner: task.launch.executable.clone(),
        directory: directory.path().join("units"),
        manager_command: manager,
        dispatch_command: dispatcher,
    };
    scheduler.sync(&store, &task).await.unwrap();
    let files = scheduler.render(&store, &task).unwrap();
    assert!(files[0].1.contains("\"trigger\""));
    assert!(!files[0].1.contains("original prompt"));
    assert!(files[1].1.contains("Persistent=false"));
    assert!(
        files[1]
            .1
            .contains("OnCalendar=Mon,Tue,Wed,Thu,Fri *-*-* 09:00:00")
    );
    assert!(
        scheduler
            .dispatch(&store, &task, RunSource::Manual)
            .await
            .is_err()
    );
    assert_eq!(
        store.runs(&task.id, None, 1).unwrap()[0].status,
        RunStatus::Failed
    );
    task.deleted = true;
    scheduler.sync(&store, &task).await.unwrap();
    assert!(!files[1].0.exists());
    scheduler.platform = Platform::Launchd;
    scheduler.runner = directory.path().join("a & b runner");
    let plist = scheduler.render(&store, &task).unwrap().remove(0).1;
    let mut python = std::process::Command::new("python3").args(["-c", "import plistlib,sys; p=plistlib.loads(sys.stdin.buffer.read()); assert p['ProgramArguments'][1]=='trigger'; assert p['RunAtLoad'] is False; assert len(p['StartCalendarInterval'])==5; assert '&' in p['ProgramArguments'][0]"])
        .stdin(Stdio::piped()).spawn().unwrap();
    python
        .stdin
        .take()
        .unwrap()
        .write_all(plist.as_bytes())
        .unwrap();
    assert!(python.wait().unwrap().success());
}

fn spawn_runner(store: &Store, task: &Task) -> (tokio::process::Child, String) {
    let id = new_run_id();
    let child = Command::new(env!("CARGO_BIN_EXE_aow-automation-runner"))
        .args([
            "run",
            "--state-dir",
            store.state_dir.to_str().unwrap(),
            "--task-id",
            &task.id,
            "--run-id",
            &id,
        ])
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    (child, id)
}
async fn session(store: &Store, task: &Task, id: &str) -> Run {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Some(run)) = store.read_run(&task.id, id)
                && run.session_id.is_some()
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
const BLOCKED_AGENT: &str = r#"
printf 'session id: session-%s\n' "$$"
cat >/dev/null
while [ ! -f "$TEST_RELEASE" ]; do sleep 0.02; done
"#;

#[tokio::test]
async fn concurrent_runs_are_bounded_and_all_running_runs_remain_visible() {
    let (directory, store, mut task) = fixture(BLOCKED_AGENT);
    assert_eq!(task.input.max_concurrent_runs, 3);
    let mut defaults = serde_json::to_value(&task.input).unwrap();
    defaults.as_object_mut().unwrap().remove("interval_seconds");
    let defaults: TaskInput = serde_json::from_value(defaults).unwrap();
    assert_eq!(defaults.max_concurrent_runs, 3);
    assert_eq!(defaults.interval_seconds, None);
    let release = directory.path().join("release");
    task.launch
        .environment
        .insert("TEST_RELEASE".into(), release.to_string_lossy().into());
    store.save_task(&task).unwrap();
    let (mut first, a) = spawn_runner(&store, &task);
    let first_run = session(&store, &task, &a).await;
    let (mut second, b) = spawn_runner(&store, &task);
    let second_run = session(&store, &task, &b).await;
    let (mut third, c) = spawn_runner(&store, &task);
    let third_run = session(&store, &task, &c).await;
    assert_ne!(first_run.session_id, second_run.session_id);
    assert_ne!(second_run.session_id, third_run.session_id);
    assert_eq!(store.runs(&task.id, None, 20).unwrap().len(), 3);
    let (mut blocked, blocked_id) = spawn_runner(&store, &task);
    assert!(blocked.wait().await.unwrap().success());
    assert!(!store.run_path(&task.id, &blocked_id).unwrap().exists());
    // Finishing one run releases one slot without hiding older runs.
    unsafe {
        libc::kill(second.id().unwrap() as i32, libc::SIGTERM);
    }
    second.wait().await.unwrap();
    assert!(store.is_running(&task.id).unwrap());
    let (mut replacement, replacement_id) = spawn_runner(&store, &task);
    session(&store, &task, &replacement_id).await;
    fs::write(&release, "done").unwrap();
    assert!(first.wait().await.unwrap().success());
    assert!(third.wait().await.unwrap().success());
    assert!(replacement.wait().await.unwrap().success());
    assert!(!store.is_running(&task.id).unwrap());
}

#[tokio::test]
async fn task_locks_allow_different_tasks_to_share_a_workspace() {
    let (directory, store, mut first_task) = fixture(BLOCKED_AGENT);
    first_task.input.max_concurrent_runs = 1;
    let release = directory.path().join("release");
    // Release the fake agents even when an assertion fails.
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "done");
        }
    }
    let release = Release(release);
    first_task
        .launch
        .environment
        .insert("TEST_RELEASE".into(), release.0.to_string_lossy().into());
    let mut second_task = first_task.clone();
    second_task.id = "task-two".into();
    store.save_task(&first_task).unwrap();
    store.save_task(&second_task).unwrap();

    let (mut first, first_id) = spawn_runner(&store, &first_task);
    let first_run = session(&store, &first_task, &first_id).await;
    let (mut second, second_id) = spawn_runner(&store, &second_task);
    let second_run = session(&store, &second_task, &second_id).await;
    assert_eq!(first_run.workspace_path, second_run.workspace_path);
    assert_ne!(first_run.session_id, second_run.session_id);
    for task in [&first_task, &second_task] {
        assert!(store.is_running(&task.id).unwrap());
        let (mut duplicate, id) = spawn_runner(&store, task);
        assert!(duplicate.wait().await.unwrap().success());
        assert!(!store.run_path(&task.id, &id).unwrap().exists());
    }
    assert!(!store.root.join("locks").exists());

    drop(release);
    assert!(first.wait().await.unwrap().success());
    assert!(second.wait().await.unwrap().success());
    for (task, id) in [(&first_task, &first_id), (&second_task, &second_id)] {
        assert_eq!(
            store.read_run(&task.id, id).unwrap().unwrap().status,
            RunStatus::Completed
        );
        assert!(!store.is_running(&task.id).unwrap());
    }
}

#[tokio::test]
async fn simultaneous_triggers_respect_one_stable_concurrency_slot() {
    use std::os::unix::fs::MetadataExt;
    let (directory, store, mut task) = fixture(BLOCKED_AGENT);
    task.input.max_concurrent_runs = 1;
    let release = directory.path().join("release");
    task.launch
        .environment
        .insert("TEST_RELEASE".into(), release.to_string_lossy().into());
    store.save_task(&task).unwrap();
    let mut children = (0..12)
        .map(|_| spawn_runner(&store, &task))
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let exited = children
                .iter_mut()
                .map(|(child, _)| child.try_wait().unwrap().is_some())
                .filter(|exited| *exited)
                .count();
            if exited == 11 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let runs = store.runs(&task.id, None, 20).unwrap();
    assert_eq!(runs.len(), 1);
    session(&store, &task, &runs[0].id).await;
    let lock = store.root.join("runs").join(&task.id).join("slot-1.lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    let owner: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&lock).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(owner["run_id"], runs[0].id);
    assert!(owner["runner_pid"].is_u64());
    let scheduler = Scheduler {
        dispatch_command: "/does-not-exist".into(),
        ..Scheduler::new(directory.path())
    };
    assert_eq!(
        scheduler
            .dispatch(&store, &task, RunSource::Scheduled)
            .await
            .unwrap(),
        None
    );
    assert_eq!(store.runs(&task.id, None, 20).unwrap().len(), 1);
    fs::write(release, "done").unwrap();
    for (mut child, _) in children {
        assert!(child.wait().await.unwrap().success());
    }
    assert_eq!(execute(&store, &task).await.status, RunStatus::Completed);
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    assert!(!store.root.join("locks").exists());
}

#[tokio::test]
async fn killed_runner_cannot_be_replaced_until_its_agent_group_exits() {
    let (directory, store, mut task) = fixture(
        r#"
cat >/dev/null
/bin/sh -c 'while [ ! -f "$TEST_RELEASE" ]; do sleep 0.02; done' &
printf 'session id: orphan-session\n'
wait
"#,
    );
    task.input.max_concurrent_runs = 1;
    let release = directory.path().join("release");
    task.launch
        .environment
        .insert("TEST_RELEASE".into(), release.to_string_lossy().into());
    store.save_task(&task).unwrap();
    let (mut process, id) = spawn_runner(&store, &task);
    let live = session(&store, &task, &id).await;
    struct Cleanup(i32);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
    }
    let _cleanup = Cleanup(live.agent_pid.unwrap() as i32);
    process.kill().await.unwrap();
    assert_eq!(
        store.read_run(&task.id, &id).unwrap().unwrap().status,
        RunStatus::Interrupted
    );
    assert!(store.is_running(&task.id).unwrap());
    // The agent's leader can disappear while its tool process is still alive.
    assert_eq!(
        unsafe { libc::kill(live.agent_pid.unwrap() as i32, libc::SIGKILL) },
        0
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(store.is_running(&task.id).unwrap());
    let (mut duplicate, duplicate_id) = spawn_runner(&store, &task);
    assert!(duplicate.wait().await.unwrap().success());
    assert!(!store.run_path(&task.id, &duplicate_id).unwrap().exists());
    // Metadata protects the tool group after both runner and agent leader exit.
    assert_eq!(
        unsafe { libc::kill(-(live.agent_pid.unwrap() as i32), 0) },
        0
    );
    fs::write(&release, "done").unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while store.is_running(&task.id).unwrap() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(execute(&store, &task).await.status, RunStatus::Completed);
    let lock = store.root.join("runs").join(&task.id).join("slot-1.lock");
    // An old PID/PGID reused by this test process must not count as that run.
    let header = fs::read_to_string(&lock)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    fs::write(
        &lock,
        format!(
            "{header}\n{{\"process_group\":{},\"started_before\":0}}\n",
            unsafe { libc::getpgrp() }
        ),
    )
    .unwrap();
    assert!(!store.is_running(&task.id).unwrap());
}

#[test]
fn intervals_render_native_timers_and_validate_bounds() {
    let (directory, store, mut task) = fixture(CODEX);
    let mut scheduler = Scheduler::new(directory.path());
    task.input.cron.clear();
    for seconds in [1, 5, 90, 300, 3600, 2_678_400] {
        task.input.interval_seconds = Some(seconds);
        task.input.validate().unwrap();
        scheduler.platform = Platform::Systemd;
        let timer = scheduler.render(&store, &task).unwrap().remove(1).1;
        assert!(timer.contains(&format!(
            "OnActiveSec={seconds}s\nOnUnitInactiveSec={seconds}s"
        )));
        assert!(!timer.contains("OnCalendar="));
        scheduler.platform = Platform::Launchd;
        let plist = scheduler.render(&store, &task).unwrap().remove(0).1;
        let mut python = std::process::Command::new("python3").args(["-c", &format!("import plistlib,sys; p=plistlib.loads(sys.stdin.buffer.read()); assert p['StartInterval']=={seconds}; assert p['ThrottleInterval']==1; assert 'StartCalendarInterval' not in p; assert p['RunAtLoad'] is False")]).stdin(Stdio::piped()).spawn().unwrap();
        python
            .stdin
            .take()
            .unwrap()
            .write_all(plist.as_bytes())
            .unwrap();
        assert!(python.wait().unwrap().success());
    }
    for seconds in [0, 2_678_401, u64::MAX] {
        task.input.interval_seconds = Some(seconds);
        assert!(task.input.validate().is_err());
        assert!(scheduler.render(&store, &task).is_err());
    }
}

fn manual_task(task: &mut Task) {
    task.input.kind = aow_automations::TaskKind::Manual;
    task.input.cron.clear();
    task.input.interval_seconds = None;
    task.input.prompt = "检查 🧪 {{分支}} / {{分支}} / {{保持原文}}".into();
    task.input.prompt_bindings = task
        .input
        .prompt
        .match_indices("{{分支}}")
        .map(|(start, placeholder)| aow_automations::PromptBinding {
            name: "分支".into(),
            placeholder: placeholder.into(),
            start,
            end: start + placeholder.len(),
        })
        .collect();
}

#[test]
fn manual_bindings_are_frozen_literal_unicode_replacements() {
    let (_directory, _store, mut task) = fixture("exit 0");
    manual_task(&mut task);
    task.input.validate().unwrap();
    let values = BTreeMap::from([("分支".into(), "feature/$1\\中文\n{{保持原文}}".into())]);
    assert_eq!(
        task.input.render_prompt(&values).unwrap(),
        "检查 🧪 feature/$1\\中文\n{{保持原文}} / feature/$1\\中文\n{{保持原文}} / {{保持原文}}"
    );
    assert!(task.input.render_prompt(&BTreeMap::new()).is_err());
    assert!(
        task.input
            .render_prompt(&BTreeMap::from([("分支".into(), "  \n".into())]))
            .is_err()
    );
    let mut extra = values.clone();
    extra.insert("未配置".into(), "value".into());
    assert!(task.input.render_prompt(&extra).is_err());
    assert!(
        task.input
            .render_prompt(&BTreeMap::from([("分支".into(), "x".repeat(65536))]))
            .is_err()
    );
    let bindings = task.input.prompt_bindings.clone();
    task.input.prompt_bindings[0].start = 1; // Inside a UTF-8 character.
    assert!(task.input.render_prompt(&values).is_err());
    task.input.prompt_bindings = bindings;
    task.input.prompt_bindings[1].start = task.input.prompt_bindings[0].start;
    assert!(task.input.validate().is_err());
    task.input.prompt_bindings.clear();
    assert_eq!(
        task.input.render_prompt(&BTreeMap::new()).unwrap(),
        task.input.prompt
    );
}

#[tokio::test]
async fn manual_tasks_have_no_timers_and_remove_previous_schedule() {
    let (directory, store, mut task) = fixture("exit 0");
    let fake = directory.path().join("manager");
    executable(&fake, "exit 0");
    let mut scheduler = Scheduler {
        platform: Platform::Systemd,
        runner: fake.clone(),
        directory: directory.path().join("units"),
        manager_command: fake.clone(),
        dispatch_command: fake,
    };
    scheduler.sync(&store, &task).await.unwrap();
    let files = scheduler.render(&store, &task).unwrap();
    manual_task(&mut task);
    scheduler.sync(&store, &task).await.unwrap();
    assert!(files.iter().all(|(path, _)| !path.exists()));
    assert!(
        store
            .task_view(task.clone())
            .unwrap()
            .state
            .next_run_at
            .is_none()
    );
    assert!(
        scheduler
            .dispatch(&store, &task, RunSource::Scheduled)
            .await
            .is_err()
    );
    scheduler.manager_command = directory.path().join("does-not-exist");
    for platform in [Platform::Systemd, Platform::Launchd, Platform::Unsupported] {
        scheduler.platform = platform;
        assert!(scheduler.render(&store, &task).unwrap().is_empty());
        scheduler.sync(&store, &task).await.unwrap();
    }
    store.save_task(&task).unwrap();
    assert_eq!(
        runner::run(&store, &task.id, None, RunSource::Scheduled)
            .await
            .unwrap(),
        RunStatus::Skipped
    );
    assert_eq!(
        runner::run(&store, &task.id, None, RunSource::Manual)
            .await
            .unwrap(),
        RunStatus::Failed
    );
}

#[tokio::test]
async fn manual_runs_use_isolated_values_and_submitted_task_snapshot() {
    let (_directory, store, mut task) = fixture("printf 'session id: manual-session\\n' >&2\ncat");
    manual_task(&mut task);
    store.save_task(&task).unwrap();
    let first = new_run_id();
    let second = new_run_id();
    for (id, value) in [(&first, "第一个"), (&second, "第二个 {{分支}} $1")] {
        store
            .save_manual_request(
                id,
                &aow_automations::ManualRunRequest {
                    task: task.clone(),
                    variables: BTreeMap::from([("分支".into(), value.into())]),
                },
            )
            .unwrap();
    }
    task.revision += 1;
    task.input.prompt = "A later edit must not change submitted runs".into();
    task.input.prompt_bindings.clear();
    store.save_task(&task).unwrap();
    let (a, b) = tokio::join!(
        runner::run(&store, &task.id, Some(first.clone()), RunSource::Manual),
        runner::run(&store, &task.id, Some(second.clone()), RunSource::Manual)
    );
    assert_eq!(
        a.unwrap(),
        RunStatus::Completed,
        "{:?}",
        store.read_run(&task.id, &first).unwrap()
    );
    assert_eq!(
        b.unwrap(),
        RunStatus::Completed,
        "{:?}",
        store.read_run(&task.id, &second).unwrap()
    );
    for (id, value) in [(&first, "第一个"), (&second, "第二个 {{分支}} $1")] {
        let output = store
            .read_run_output(&task.id, id, aow_automations::RunOutput::Stdio)
            .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("检查 🧪 {value} / {value} / {{{{保持原文}}}}")
        );
        assert_eq!(
            store.read_run(&task.id, id).unwrap().unwrap().task_revision,
            1
        );
        let reopened = Store::open(store.state_dir.clone()).unwrap();
        let detail = reopened.read_run_detail(&task.id, id).unwrap().unwrap();
        assert_eq!(
            detail.run.variables,
            Some(BTreeMap::from([("分支".into(), value.into())]))
        );
        assert_eq!(detail.configuration.prompt_bindings.len(), 2);
        assert_eq!(
            detail.configuration.prompt,
            "检查 🧪 {{分支}} / {{分支}} / {{保持原文}}"
        );
        assert!(store.take_manual_request(&task.id, id).unwrap().is_none());
    }
    assert_eq!(
        store.get_task(&task.id).unwrap().input.prompt,
        task.input.prompt
    );
}

#[tokio::test]
async fn manual_run_parameters_survive_dispatch_failure_and_legacy_history_remains_readable() {
    let (directory, store, mut task) = fixture("exit 0");
    manual_task(&mut task);
    let variables = BTreeMap::from([("分支".into(), "feature/中文\n{{分支}} $1".into())]);
    let scheduler = Scheduler {
        runner: directory.path().join("missing-runner"),
        ..Scheduler::new(directory.path())
    };
    assert!(
        scheduler
            .dispatch_with_variables(&store, &task, RunSource::Manual, variables.clone())
            .await
            .is_err()
    );
    let run = store.runs(&task.id, None, 1).unwrap().remove(0);
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.variables.as_ref(), Some(&variables));
    assert!(
        store
            .take_manual_request(&task.id, &run.id)
            .unwrap()
            .is_none()
    );
    let path = store
        .run_path(&task.id, &run.id)
        .unwrap()
        .join("events.jsonl");
    let legacy = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut event: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(run) = event
                .get_mut("run")
                .and_then(serde_json::Value::as_object_mut)
            {
                run.remove("variables");
            }
            serde_json::to_string(&event).unwrap() + "\n"
        })
        .collect::<String>();
    fs::write(path, legacy).unwrap();
    assert_eq!(
        store
            .read_run(&task.id, &run.id)
            .unwrap()
            .unwrap()
            .variables,
        None
    );
    let empty = runner::initial_run(&task, new_run_id(), RunSource::Manual);
    drop(store.create_run(&empty, &task).unwrap());
    assert_eq!(
        store
            .read_run(&task.id, &empty.id)
            .unwrap()
            .unwrap()
            .variables,
        Some(BTreeMap::new())
    );
}
