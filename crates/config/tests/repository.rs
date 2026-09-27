use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};

use aow_config::{
    CONFIG_FILE, ConfigRepository, ConfigSelection, configuration_directory, inspect_repository,
    save_selection,
};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn migration_copies_only_configuration_and_preserves_originals() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state with spaces");
    fs::create_dir_all(state.join("automations/tasks")).unwrap();
    let settings = br#"{"version":1,"notes_base":"/notes"}"#;
    fs::write(state.join("aow-settings.json"), settings).unwrap();
    fs::write(
        state.join("automations/tasks/12345678.json"),
        b"{\"id\":\"12345678\"}",
    )
    .unwrap();
    for path in [
        "pin.md5",
        "terminals.json",
        "session-shares.json",
        "clipboard-images/image.png",
        "automations/runs/log",
        "repos/__aow_floating/.git/HEAD",
    ] {
        let path = state.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"local runtime data").unwrap();
    }
    let config = ConfigRepository::initialize(&state).unwrap();
    let repo = config.directory().parent().unwrap();
    assert_eq!(fs::read(state.join("aow-settings.json")).unwrap(), settings);
    assert_eq!(
        fs::read(config.directory().join("aow-settings.json")).unwrap(),
        settings
    );
    assert!(!state.join("__current__").exists());
    let document = fs::read_to_string(state.join(CONFIG_FILE))
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    assert_eq!(document["config-repo"].as_str(), repo.to_str());
    assert_eq!(
        document["config-id"].as_str(),
        config.directory().file_name().unwrap().to_str()
    );
    assert_eq!(document.len(), 2);
    let files = git(repo, &["ls-files"]);
    assert_eq!(files.lines().count(), 4, "{files}");
    for forbidden in [
        "pin.md5",
        "terminals",
        "shares",
        "clipboard",
        "runs",
        "repos",
        "__current__",
        CONFIG_FILE,
    ] {
        assert!(!files.contains(forbidden), "{files}");
    }
    assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "1");
    git(repo, &["branch", "-m", "existing"]);
    assert_eq!(
        ConfigRepository::initialize(&state).unwrap().directory(),
        config.directory()
    );
    assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), "refs/heads/existing");
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "1");
    assert!(git(repo, &["status", "--porcelain"]).is_empty());
}

#[test]
fn saves_commit_changes_skip_noops_and_preserve_unrelated_staging() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let repo = config.directory().parent().unwrap();
    fs::write(repo.join("unrelated.json"), b"{}").unwrap();
    git(repo, &["add", "unrelated.json"]);
    for content in [
        b"{\"notes_base\":\"/a\"}".as_slice(),
        b"{\"notes_base\":\"/b\"}".as_slice(),
    ] {
        config
            .save(Path::new("aow-settings.json"), content)
            .unwrap();
        config
            .save(Path::new("aow-settings.json"), content)
            .unwrap();
    }
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "3");
    assert_eq!(
        git(repo, &["diff", "--cached", "--name-only"]),
        "unrelated.json"
    );
    assert!(!git(repo, &["ls-tree", "-r", "--name-only", "HEAD"]).contains("unrelated.json"));
    let file = format!(
        "HEAD:{}/aow-settings.json",
        config.directory().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(git(repo, &["show", &file]), "{\"notes_base\":\"/b\"}");
}

#[test]
fn failed_commit_preserves_saved_file_and_retry_records_it() {
    let state = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    let repo = config.directory().parent().unwrap();
    let index_lock = repo.join(".git/index.lock");
    fs::write(&index_lock, b"another git operation").unwrap();
    let error = config
        .save(Path::new("aow-settings.json"), b"{}")
        .unwrap_err();
    assert!(error.to_string().contains("Git 提交失败"));
    assert_eq!(
        fs::read(config.directory().join("aow-settings.json")).unwrap(),
        b"{}"
    );
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "1");
    fs::remove_file(index_lock).unwrap();
    config.save(Path::new("aow-settings.json"), b"{}").unwrap();
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "2");
    assert!(git(repo, &["status", "--porcelain"]).is_empty());
}

#[test]
fn external_clone_is_selected_without_creating_a_default_repository() {
    let root = tempfile::tempdir().unwrap();
    let source = ConfigRepository::initialize(&root.path().join("source-state")).unwrap();
    let repository = source.directory().parent().unwrap();
    let cloned = root.path().join("external clone");
    git(
        root.path(),
        &[
            "clone",
            repository.to_str().unwrap(),
            cloned.to_str().unwrap(),
        ],
    );
    let state = root.path().join("new-state");
    fs::create_dir(&state).unwrap();
    let selected = cloned.join(source.directory().file_name().unwrap());
    let selected = fs::canonicalize(selected).unwrap();
    save_selection(
        &state,
        &ConfigSelection {
            config_repo: cloned.clone(),
            config_id: source.selection().config_id,
        },
    )
    .unwrap();
    let document = fs::read(state.join(CONFIG_FILE)).unwrap();
    assert_eq!(configuration_directory(&state).unwrap(), selected);
    assert_eq!(fs::read(state.join(CONFIG_FILE)).unwrap(), document);
    let config = ConfigRepository::initialize(&state).unwrap();
    assert!(state.join(CONFIG_FILE).exists());
    assert!(!state.join("__current__").exists());
    config
        .save(Path::new("automations/tasks/task-one.json"), b"{}")
        .unwrap();
    assert_eq!(config.directory(), selected);
    assert_eq!(configuration_directory(&state).unwrap(), selected);
    assert!(!state.join("config-repo").exists());
    assert_eq!(git(repository, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git(&cloned, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn linked_worktree_uses_the_common_repository_lock() {
    let root = tempfile::tempdir().unwrap();
    let source = ConfigRepository::initialize(&root.path().join("source-state")).unwrap();
    let repository = source.directory().parent().unwrap();
    let linked = root.path().join("linked worktree");
    git(
        repository,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let state = root.path().join("linked-state");
    fs::create_dir(&state).unwrap();
    let selected = linked.join(source.directory().file_name().unwrap());
    let selected = fs::canonicalize(selected).unwrap();
    save_selection(
        &state,
        &ConfigSelection {
            config_repo: linked.clone(),
            config_id: source.selection().config_id,
        },
    )
    .unwrap();
    let lock = repository.join(".git/aow-config.lock");
    fs::remove_file(&lock).unwrap();

    let config = ConfigRepository::initialize(&state).unwrap();
    config.save(Path::new("aow-settings.json"), b"{}").unwrap();

    assert!(lock.is_file());
    assert_eq!(config.directory(), selected);
    assert_eq!(git(repository, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git(&linked, &["rev-list", "--count", "HEAD"]), "2");
    assert!(git(&linked, &["status", "--porcelain"]).is_empty());
}

#[test]
fn obsolete_selector_is_ignored_without_initializing_on_read() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    assert_eq!(configuration_directory(&missing).unwrap(), missing);
    assert!(!missing.exists());
    let source = ConfigRepository::initialize(&root.path().join("source")).unwrap();
    for selection in [
        source.directory().to_str().unwrap(),
        "",
        "relative/path",
        "/missing/550e8400-e29b-41d4-a716-446655440000",
        "/a\n/b",
    ] {
        fs::write(root.path().join("__current__"), selection).unwrap();
        assert!(ConfigRepository::open(root.path()).unwrap().is_none());
        assert_eq!(configuration_directory(root.path()).unwrap(), root.path());
        assert_eq!(
            fs::read_to_string(root.path().join("__current__")).unwrap(),
            selection
        );
        assert!(!root.path().join("config-repo").exists());
        assert!(!root.path().join(CONFIG_FILE).exists());
    }
    fs::remove_file(root.path().join("__current__")).unwrap();
    std::os::unix::fs::symlink(&missing, root.path().join("__current__")).unwrap();
    assert!(ConfigRepository::open(root.path()).unwrap().is_none());
    let initialized = ConfigRepository::initialize(root.path()).unwrap();
    assert_eq!(
        initialized.selection().config_repo,
        fs::canonicalize(root.path().join("config-repo")).unwrap()
    );
    assert!(root.path().join("__current__").is_symlink());
    // Users can select the previous repository explicitly; the obsolete file
    // is not consulted or deleted when the new selection is saved.
    save_selection(root.path(), &source.selection()).unwrap();
    assert_eq!(
        configuration_directory(root.path()).unwrap(),
        source.directory()
    );
    assert!(root.path().join("__current__").is_symlink());
}

#[test]
fn corrupt_legacy_json_does_not_publish_a_selection_or_change_the_source() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("aow-settings.json"), b"broken").unwrap();
    assert!(ConfigRepository::initialize(root.path()).is_err());
    assert!(!root.path().join("__current__").exists());
    assert!(!root.path().join(CONFIG_FILE).exists());
    assert_eq!(
        fs::read(root.path().join("aow-settings.json")).unwrap(),
        b"broken"
    );
}

#[test]
fn runtime_paths_and_symlink_destinations_are_rejected() {
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(state.path()).unwrap();
    for path in [
        "pin.md5",
        "terminals.json",
        "automations/runs/test.json",
        "../aow-settings.json",
    ] {
        assert!(config.save(Path::new(path), b"{}").is_err());
    }
    std::os::unix::fs::symlink(outside.path(), config.directory().join("automations")).unwrap();
    assert!(
        config
            .save(Path::new("automations/tasks/task-one.json"), b"{}")
            .is_err()
    );
    assert!(!outside.path().join("tasks/task-one.json").exists());
}

#[test]
fn repository_discovery_validates_roots_and_selects_uuid_input() {
    let root = tempfile::tempdir().unwrap();
    let config = ConfigRepository::initialize(root.path()).unwrap();
    let selection = config.selection();
    let second = "550e8400-e29b-41d4-a716-446655440000";
    fs::create_dir(selection.config_repo.join(second)).unwrap();
    fs::create_dir(selection.config_repo.join("not-a-version")).unwrap();
    fs::write(
        selection
            .config_repo
            .join("550e8400-e29b-41d4-a716-446655440001"),
        "file",
    )
    .unwrap();
    let choices = inspect_repository(&selection.config_repo).unwrap();
    let mut expected = vec![selection.config_id.clone(), second.to_owned()];
    expected.sort();
    assert_eq!(choices.config_ids, expected);
    assert_eq!(choices.selected_id, None);
    let direct = inspect_repository(config.directory()).unwrap();
    assert_eq!(direct.config_repo, selection.config_repo);
    assert_eq!(
        direct.selected_id.as_deref(),
        Some(selection.config_id.as_str())
    );
    assert_eq!(direct.config_ids, choices.config_ids);
    let orphan = root.path().join(second);
    fs::create_dir(&orphan).unwrap();
    for invalid in [
        root.path().to_path_buf(),
        orphan,
        selection.config_repo.join("not-a-version"),
        root.path().join("missing"),
        Path::new("relative").to_path_buf(),
    ] {
        assert!(
            inspect_repository(&invalid).is_err(),
            "{}",
            invalid.display()
        );
    }
    fs::remove_dir(selection.config_repo.join(second)).unwrap();
    fs::remove_dir_all(config.directory()).unwrap();
    assert!(
        inspect_repository(&selection.config_repo)
            .unwrap()
            .config_ids
            .is_empty()
    );
}

#[test]
fn toml_selection_switches_on_reopen_preserves_extra_fields_and_skips_noops() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let active = ConfigRepository::initialize(&state).unwrap();
    let target =
        ConfigRepository::initialize(&root.path().join("other ' quoted repository")).unwrap();
    let path = state.join(CONFIG_FILE);
    let original = format!(
        "{}\n# Future settings\n[project]\nname = 'keep me'\n",
        fs::read_to_string(&path).unwrap()
    );
    fs::write(&path, &original).unwrap();
    let (selection, changed) = save_selection(&state, &active.selection()).unwrap();
    assert_eq!(selection, active.selection());
    assert!(!changed);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(!save_selection(&state, &selection).unwrap().1);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert!(save_selection(&state, &target.selection()).unwrap().1);
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("# Future settings\n[project]\nname = 'keep me'"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(configuration_directory(&state).unwrap(), target.directory());
    active
        .save(Path::new("aow-settings.json"), b"{\"active\":true}")
        .unwrap();
    assert!(!target.directory().join("aow-settings.json").exists());
    assert_eq!(
        ConfigRepository::initialize(&state).unwrap().directory(),
        target.directory()
    );
    for invalid in [
        ConfigSelection {
            config_id: "../bad".into(),
            ..selection.clone()
        },
        ConfigSelection {
            config_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            ..selection.clone()
        },
        ConfigSelection {
            config_repo: root.path().to_path_buf(),
            ..selection.clone()
        },
    ] {
        assert!(save_selection(&state, &invalid).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
    // A failed atomic replacement does not alter either version's configuration.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(save_selection(&state, &selection).is_err());
    assert_eq!(
        fs::read(active.directory().join("aow-settings.json")).unwrap(),
        b"{\"active\":true}"
    );
}

#[test]
fn invalid_toml_never_falls_back_to_old_selector_or_initializes_a_repository() {
    let root = tempfile::tempdir().unwrap();
    let source = ConfigRepository::initialize(&root.path().join("source")).unwrap();
    let state = root.path().join("state");
    fs::create_dir(&state).unwrap();
    fs::write(
        state.join("__current__"),
        source.directory().to_str().unwrap(),
    )
    .unwrap();
    for text in [
        "",
        "bad = [",
        "config-repo = 42",
        "config-repo = '/tmp'\nconfig-id = 'invalid'",
        "config-repo = 'relative'\nconfig-id = '550e8400-e29b-41d4-a716-446655440000'",
    ] {
        fs::write(state.join(CONFIG_FILE), text).unwrap();
        assert!(ConfigRepository::open(&state).is_err());
        assert!(ConfigRepository::initialize(&state).is_err());
        assert_eq!(fs::read_to_string(state.join(CONFIG_FILE)).unwrap(), text);
        assert!(state.join("__current__").exists());
        assert!(!state.join("config-repo").exists());
    }
    fs::remove_file(state.join(CONFIG_FILE)).unwrap();
    std::os::unix::fs::symlink(state.join("missing"), state.join(CONFIG_FILE)).unwrap();
    assert!(ConfigRepository::initialize(&state).is_err());
}

// Invoked as separate OS processes below to exercise the repository file lock.
#[test]
fn child_writer() {
    let Some(state) = std::env::var_os("AOW_CONFIG_TEST_STATE") else {
        return;
    };
    let slot = std::env::var("AOW_CONFIG_TEST_SLOT").unwrap();
    let config = ConfigRepository::initialize(Path::new(&state)).unwrap();
    for revision in 0..3 {
        config
            .save(
                Path::new(&format!("automations/tasks/{slot}.json")),
                format!("{{\"revision\":{revision}}}").as_bytes(),
            )
            .unwrap();
    }
}

#[test]
fn initialization_and_saves_do_not_require_recent_git_options() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
for arg do
    case "$arg" in
        --initial-branch*|-b|--path-format*)
            printf 'unsupported Git option: %s\n' "$arg" >&2
            exit 129
            ;;
    esac
done
PATH="$AOW_CONFIG_TEST_ORIGINAL_PATH" exec git "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let original_path = std::env::var_os("PATH").unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&original_path)))
            .unwrap();
    let global_config = root.path().join("gitconfig");
    fs::write(&global_config, "[init]\n\tdefaultBranch = custom\n").unwrap();
    let state = root.path().join("state");
    // Use a child process so overriding PATH cannot affect parallel tests.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_writer", "--nocapture"])
        .env("PATH", path)
        .env("AOW_CONFIG_TEST_ORIGINAL_PATH", original_path)
        .env("GIT_CONFIG_GLOBAL", global_config)
        .env("AOW_CONFIG_TEST_STATE", &state)
        .env("AOW_CONFIG_TEST_SLOT", "legacy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let config = ConfigRepository::open(&state).unwrap().unwrap();
    let repository = config.directory().parent().unwrap();
    assert_eq!(
        git(repository, &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(git(repository, &["rev-list", "--count", "HEAD"]), "4");
    assert!(git(repository, &["status", "--porcelain"]).is_empty());
}

#[test]
fn processes_share_initialization_and_serialize_all_commits() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut children = Vec::new();
    for slot in 0..4 {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_writer", "--nocapture"])
                .env("AOW_CONFIG_TEST_STATE", &state)
                .env("AOW_CONFIG_TEST_SLOT", slot.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let config = ConfigRepository::open(&state).unwrap().unwrap();
    let repo = config.directory().parent().unwrap();
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "13");
    assert_eq!(
        fs::read_dir(repo)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name() != ".git")
            .count(),
        1
    );
    assert!(git(repo, &["status", "--porcelain"]).is_empty());
    for slot in 0..4 {
        assert_eq!(
            fs::read_to_string(
                config
                    .directory()
                    .join(format!("automations/tasks/{slot}.json"))
            )
            .unwrap(),
            "{\"revision\":2}"
        );
    }
}
