use super::*;
use aow_agents::environment;
use std::{
    collections::{BTreeMap, HashSet},
    os::unix::fs::PermissionsExt,
    process::Command as StdCommand,
};

#[tokio::test]
async fn node_settings_persist_clear_and_preserve_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let notes_base = directory.path().join("notes");
    std::fs::write(
        directory.path().join(SETTINGS_FILE),
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "notes_base": notes_base, "execution_path": ["/usr/bin"],
            "editor": { "word_wrap": true }, "pinned_worktrees": ["/repo/main"],
            "pinned_worktrees_revision": 7,
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    assert!(manager.settings().unwrap().node_addresses.is_empty());
    let update = serde_json::from_value(serde_json::json!({
        "node_addresses": [" HTTPS://NODE-A.EXAMPLE:443 ", "", "https://node-a.example/",
            "http://192.168.1.2:8080/aow/?ui=desktop#tab", "http://[::1]:8080"]
    }))
    .unwrap();
    let saved = manager.update_settings(update).await.unwrap();
    let expected = vec![
        "https://node-a.example/",
        "http://192.168.1.2:8080/aow/?ui=desktop#tab",
        "http://[::1]:8080/",
    ];
    assert_eq!(saved.node_addresses, expected);
    assert_eq!(Path::new(&saved.notes_base), notes_base);
    assert!(saved.editor.word_wrap);
    assert_eq!(saved.pinned_worktrees, ["/repo/main"]);
    assert_eq!(saved.pinned_worktrees_revision, 7);
    assert_eq!(
        saved.execution_path.unwrap(),
        vec![PathBuf::from("/usr/bin")]
    );
    manager
        .update_settings(UpdateSettingsRequest {
            editor: Some(EditorSettings { word_wrap: false }),
            ..Default::default()
        })
        .await
        .unwrap();
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert_eq!(reloaded.settings().unwrap().node_addresses, expected);
    reloaded
        .update_settings(UpdateSettingsRequest {
            node_addresses: Some(Vec::new()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        AowManager::persistent(directory.path())
            .unwrap()
            .settings()
            .unwrap()
            .node_addresses
            .is_empty()
    );
}

#[tokio::test]
async fn node_settings_invalid_input_and_failed_persistence_keep_previous_value() {
    let directory = tempfile::tempdir().unwrap();
    let manager =
        AowManager::persistent_with_notes_base(directory.path(), directory.path().join("notes"))
            .unwrap();
    for address in [
        "node.example",
        "javascript:alert(1)",
        "file:///tmp/node",
        "https:node/path://",
        "https://user:password@node.example",
    ] {
        assert!(
            manager
                .update_settings(UpdateSettingsRequest {
                    node_addresses: Some(vec!["https://valid.example".into(), address.into()]),
                    ..Default::default()
                })
                .await
                .is_err(),
            "accepted invalid address: {address}"
        );
        assert!(manager.settings().unwrap().node_addresses.is_empty());
    }
    std::fs::create_dir(manager.inner.settings_path.as_ref().unwrap()).unwrap();
    assert!(
        manager
            .update_settings(UpdateSettingsRequest {
                node_addresses: Some(vec!["https://node.example".into()]),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(manager.settings().unwrap().node_addresses.is_empty());
}

#[tokio::test]
async fn editor_settings_default_persist_and_preserve_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let notes_base = directory.path().join("notes");
    // A settings file written before Editor preferences existed still loads.
    std::fs::write(
        directory.path().join(SETTINGS_FILE),
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "notes_base": notes_base, "execution_path": ["/usr/bin"],
            "pinned_worktrees": ["/repo/main"], "pinned_worktrees_revision": 7,
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    assert!(!manager.settings().unwrap().editor.word_wrap);
    let update = serde_json::from_value(serde_json::json!({
        "editor": { "word_wrap": true }
    }))
    .unwrap();
    let saved = manager.update_settings(update).await.unwrap();
    assert!(saved.editor.word_wrap);
    assert_eq!(Path::new(&saved.notes_base), notes_base);
    assert_eq!(
        saved.execution_path.unwrap(),
        vec![PathBuf::from("/usr/bin")]
    );
    assert_eq!(saved.pinned_worktrees, ["/repo/main"]);
    assert_eq!(saved.pinned_worktrees_revision, 7);

    manager
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![PathBuf::from("/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert!(reloaded.settings().unwrap().editor.word_wrap);
    assert_eq!(
        reloaded.execution_path().await.unwrap(),
        vec![PathBuf::from("/bin")]
    );
    reloaded
        .update_settings(UpdateSettingsRequest {
            editor: Some(EditorSettings { word_wrap: false }),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        !AowManager::persistent(directory.path())
            .unwrap()
            .settings()
            .unwrap()
            .editor
            .word_wrap
    );
    assert!(
        serde_json::from_value::<UpdateSettingsRequest>(serde_json::json!({
            "editor": { "word_wrap": "on" }
        }))
        .is_err()
    );
}

#[tokio::test]
async fn editor_settings_failed_persistence_retains_previous_value() {
    let directory = tempfile::tempdir().unwrap();
    let manager =
        AowManager::persistent_with_notes_base(directory.path(), directory.path().join("notes"))
            .unwrap();
    std::fs::create_dir(manager.inner.settings_path.as_ref().unwrap()).unwrap();
    assert!(
        manager
            .update_settings(UpdateSettingsRequest {
                editor: Some(EditorSettings { word_wrap: true }),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(!manager.settings().unwrap().editor.word_wrap);
}

#[tokio::test]
async fn execution_path_defaults_persist_and_settings_updates_preserve_other_fields() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent_with_notes_base(
        directory.path(),
        directory.path().join("original notes"),
    )
    .unwrap();
    let initial = manager.execution_path().await.unwrap();
    assert!(!initial.is_empty());
    assert_eq!(
        environment::load_path(directory.path()).await.unwrap(),
        initial
    );
    let bin = directory.path().join("custom bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("codex"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(bin.join("codex"), std::fs::Permissions::from_mode(0o700)).unwrap();
    manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/main".into()],
            ..Default::default()
        })
        .unwrap();
    let paths = vec![bin.clone(), PathBuf::from("/usr/bin")];
    let settings = manager
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![bin.clone(), bin.clone(), "/usr/bin".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(settings.execution_path.as_ref().unwrap(), &paths);
    assert_eq!(
        Path::new(&settings.notes_base),
        directory.path().join("original notes")
    );
    assert_eq!(
        manager
            .agents()
            .await
            .unwrap()
            .iter()
            .find(|agent| agent.id == "codex")
            .unwrap()
            .executable
            .as_deref(),
        bin.join("codex").to_str()
    );
    manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(
                directory
                    .path()
                    .join("new notes")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..Default::default()
        })
        .await
        .unwrap();
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert_eq!(reloaded.execution_path().await.unwrap(), paths);
    assert_eq!(
        environment::load_path(directory.path()).await.unwrap(),
        paths
    );
    assert_eq!(reloaded.pinned_worktrees().unwrap().paths, ["/repo/main"]);
    for invalid in [
        vec![],
        vec!["relative/bin".into()],
        vec!["/bin:/usr/bin".into()],
        vec![bin.join("codex")],
    ] {
        assert!(
            manager
                .update_settings(UpdateSettingsRequest {
                    execution_path: Some(invalid),
                    ..Default::default()
                })
                .await
                .is_err()
        );
        assert_eq!(
            environment::load_path(directory.path()).await.unwrap(),
            paths
        );
    }
}

#[test]
fn parses_git_worktree_porcelain() {
    let worktrees = worktrees::parse_worktrees(
        "project-1",
        "worktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo-feature\nHEAD def456\ndetached\nlocked reason\nprunable stale\n\n",
    );
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].path, "/repo");
    assert_eq!(worktrees[0].branch, "main");
    assert!(worktrees[0].is_main);
    assert!(worktrees[1].detached);
    assert!(worktrees[1].locked);
    assert!(worktrees[1].prunable);
}

#[test]
fn registered_projects_require_a_notes_path_and_default_worktree_colors() {
    let missing_notes = serde_json::from_value::<StoredProject>(serde_json::json!({
        "id": "project-1",
        "name": "Project",
        "registered_path": "/repo",
        "common_git_dir": "/repo/.git"
    }));
    assert!(missing_notes.is_err());

    let project: StoredProject = serde_json::from_value(serde_json::json!({
        "id": "project-1",
        "name": "Project",
        "registered_path": "/repo",
        "common_git_dir": "/repo/.git",
        "notes_path": "/notes/project-1"
    }))
    .unwrap();
    assert!(project.worktree_colors.is_empty());
    assert!(project.worktree_icons.is_empty());
    assert!(
        serde_json::from_value::<SetWorktreeIconRequest>(serde_json::json!({
            "path": "/repo", "icon": "unknown"
        }))
        .is_err()
    );
}

#[test]
fn maps_supported_remote_urls_to_the_same_notes_identity() {
    let expected = PathBuf::from("github.com/openai/codex");
    assert_eq!(
        notes::parse_remote_identity("git@github.com:openai/codex.git").unwrap(),
        expected
    );
    assert_eq!(
        notes::parse_remote_identity("https://github.com/openai/codex.git").unwrap(),
        expected
    );
    assert_eq!(
        notes::parse_remote_identity("ssh://git@github.com/openai/codex.git").unwrap(),
        expected
    );
    assert_eq!(
        notes::parse_remote_identity("https://git.example.com/group/subgroup/repo.git").unwrap(),
        PathBuf::from("git.example.com/group/subgroup/repo")
    );
}

#[test]
fn rejects_unsafe_remote_paths() {
    assert!(notes::parse_remote_identity("file:///tmp/repo").is_err());
    assert!(notes::parse_remote_identity("git@example.com:../repo.git").is_err());
}

#[test]
fn maps_local_repository_names_to_localhost_notes_identities() {
    assert_eq!(
        notes::local_repository_identity(Path::new("/projects/example"), "alice").unwrap(),
        PathBuf::from("localhost/alice/example")
    );
    assert!(notes::local_repository_identity(Path::new("/projects/example"), "../alice").is_err());
}

#[tokio::test]
async fn pinned_directories_persist_merge_and_preserve_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let notes_base = directory.path().join("notes");
    std::fs::write(directory.path().join(SETTINGS_FILE), serde_json::to_vec(&serde_json::json!({
        "version": 1, "notes_base": notes_base, "pinned_worktrees": ["/repo/main"], "pinned_worktrees_revision": 7,
    })).unwrap()).unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    assert!(manager.pinned_directories().unwrap().paths.is_empty());
    let first = directory.path().join("目录 #1");
    let second = directory.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let first = first.to_string_lossy().into_owned();
    let second = second.to_string_lossy().into_owned();
    let (one, two) = tokio::join!(
        manager.update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec![first.clone(), format!("{first}/")],
            ..Default::default()
        }),
        manager.update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec![second.clone()],
            ..Default::default()
        }),
    );
    one.unwrap();
    two.unwrap();
    let pins = manager.pinned_directories().unwrap();
    assert_eq!(pins.paths.len(), 2);
    assert!(pins.paths.contains(&first));
    assert!(pins.paths.contains(&second));
    assert_eq!(pins.revision, 2);
    let duplicate = manager
        .update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec![first.clone()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(duplicate.revision, pins.revision);
    let configured_notes = directory.path().join("configured-notes");
    manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(configured_notes.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/feature".into()],
            ..Default::default()
        })
        .unwrap();
    drop(manager);
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert_eq!(reloaded.pinned_directories().unwrap().paths, pins.paths);
    assert_eq!(
        reloaded.pinned_directories().unwrap().revision,
        pins.revision
    );
    assert_eq!(
        reloaded.pinned_worktrees().unwrap().paths,
        ["/repo/main", "/repo/feature"]
    );
    assert_eq!(
        Path::new(&reloaded.settings().unwrap().notes_base),
        configured_notes
    );
    std::fs::remove_dir(&first).unwrap();
    let remaining = reloaded
        .update_pinned_directories(UpdatePinnedDirectoriesRequest {
            remove: vec![first],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(remaining.paths, [second]);
    assert_eq!(remaining.revision, 3);
    drop(reloaded);
    assert_eq!(
        AowManager::persistent(directory.path())
            .unwrap()
            .pinned_directories()
            .unwrap()
            .paths,
        remaining.paths
    );
}

#[tokio::test]
async fn pinned_directories_roll_back_when_config_write_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    let previous = manager
        .update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec!["/".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let settings_path = manager.inner.settings_path.as_ref().unwrap();
    std::fs::remove_file(&settings_path).unwrap();
    std::fs::create_dir(&settings_path).unwrap();
    assert!(
        manager
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                remove: vec!["/".into()],
                ..Default::default()
            })
            .await
            .is_err()
    );
    let unchanged = manager.pinned_directories().unwrap();
    assert_eq!(unchanged.paths, previous.paths);
    assert_eq!(unchanged.revision, previous.revision);
}

#[tokio::test]
async fn pinned_directories_routes_validate_paths_and_reject_files() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let directory = tempfile::tempdir().unwrap();
    let regular_file = directory.path().join("file.txt");
    std::fs::write(&regular_file, "not a directory").unwrap();
    let app = crate::build_router(AppState::new(directory.path().to_path_buf()));
    for invalid in [
        "relative/path",
        "/bad\0path",
        regular_file.to_str().unwrap(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::patch("/api/aow/pinned-directories")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "add": [invalid] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    for (request, expected) in [
        (
            serde_json::json!({ "add": ["/"] }),
            serde_json::json!(["/"]),
        ),
        (
            serde_json::json!({ "remove": ["/"] }),
            serde_json::json!([]),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::patch("/api/aow/pinned-directories")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/aow/pinned-directories")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(payload["paths"], expected);
    }
}

#[tokio::test]
async fn pinned_worktrees_persist_and_merge_without_losing_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let notes_base = directory.path().join("notes");
    let settings_path = directory.path().join(SETTINGS_FILE);
    std::fs::write(
        &settings_path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "notes_base": notes_base,
        }))
        .unwrap(),
    )
    .unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    assert!(manager.pinned_worktrees().unwrap().paths.is_empty());
    let pins = manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/b".into(), "/repo/a".into(), "/repo/b".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pins.paths, ["/repo/b", "/repo/a"]);
    manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/c".into()],
            ..Default::default()
        })
        .unwrap();
    let pins = manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            order: Some(vec![
                "/repo/a".into(),
                "/repo/b".into(),
                "/repo/missing".into(),
            ]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pins.paths, ["/repo/a", "/repo/b", "/repo/c"]);
    let pins = manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            remove: vec!["/repo/b".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pins.paths, ["/repo/a", "/repo/c"]);
    let configured_notes = directory.path().join("configured-notes");
    manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(configured_notes.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    drop(manager);
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert_eq!(reloaded.pinned_worktrees().unwrap().paths, pins.paths);
    assert_eq!(reloaded.pinned_worktrees().unwrap().revision, pins.revision);
    assert_eq!(
        Path::new(&reloaded.settings().unwrap().notes_base),
        configured_notes
    );
    assert!(
        reloaded
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["relative/path".into()],
                ..Default::default()
            })
            .is_err()
    );
}

#[test]
fn pinned_worktrees_roll_back_when_config_write_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    let previous = manager
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/a".into()],
            ..Default::default()
        })
        .unwrap();
    let settings_path = manager.inner.settings_path.as_ref().unwrap();
    std::fs::remove_file(&settings_path).unwrap();
    std::fs::create_dir(&settings_path).unwrap();
    assert!(
        manager
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/b".into()],
                ..Default::default()
            })
            .is_err()
    );
    let unchanged = manager.pinned_worktrees().unwrap();
    assert_eq!(unchanged.paths, previous.paths);
    assert_eq!(unchanged.revision, previous.revision);
}

#[tokio::test]
async fn persists_configured_notes_base_and_uses_it_for_new_projects() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repo");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["remote", "add", "origin", "git@github.com:openai/codex.git"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    let state_directory = directory.path().join("state");
    let notes_base = directory.path().join("configured-notes");
    let manager = AowManager::persistent_with_notes_base(
        &state_directory,
        directory.path().join("default-notes"),
    )
    .unwrap();

    let settings = manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(notes_base.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(Path::new(&settings.notes_base), notes_base);
    assert!(notes_base.is_dir());

    let project = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    assert_eq!(
        Path::new(&project.notes_path),
        notes_base.join("github.com/openai/codex")
    );

    drop(manager);
    let reloaded = AowManager::persistent_with_notes_base(
        &state_directory,
        directory.path().join("another-default"),
    )
    .unwrap();
    assert_eq!(
        Path::new(&reloaded.settings().unwrap().notes_base),
        notes_base
    );
}

#[tokio::test]
async fn allows_notes_base_nested_in_a_git_repository() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    let nested = repository.join("notes");
    let result = settings::prepare_notes_base_directory(&nested)
        .await
        .unwrap();
    assert_eq!(result, nested);
    assert!(nested.is_dir());
}

#[test]
fn generates_short_note_ids_from_the_safe_alphabet() {
    let first = notes::short_random_id();
    let second = notes::short_random_id();
    assert_eq!(first.len(), 8);
    assert_eq!(second.len(), 8);
    assert_ne!(first, second);
    assert!(
        first
            .chars()
            .all(|character| { "23456789abcdefghjkmnpqrstuvwxyz".contains(character) })
    );
}

#[tokio::test]
async fn retries_a_colliding_temporary_note_name_without_scanning() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join(".tmp-aaaaaaaa.md"), b"existing").unwrap();
    let mut ids = ["aaaaaaaa", "bbbbbbbb"].into_iter();

    let (path, name) =
        notes::create_temporary_note_file(directory.path(), "md", || ids.next().unwrap().into())
            .await
            .unwrap();

    assert_eq!(name, ".tmp-bbbbbbbb.md");
    assert_eq!(path, directory.path().join(&name));
    assert_eq!(
        std::fs::read(directory.path().join(".tmp-aaaaaaaa.md")).unwrap(),
        b"existing"
    );
}

#[tokio::test]
async fn registers_and_reuses_a_remote_mapped_notes_directory() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repo");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["remote", "add", "origin", "git@github.com:openai/codex.git"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );
    let manager = AowManager::persistent_with_notes_base(
        &directory.path().join("state"),
        directory.path().join("aow"),
    )
    .unwrap();
    let project = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let expected = directory.path().join("aow/github.com/openai/codex");
    assert_eq!(Path::new(&project.notes_path), expected);
    assert!(expected.is_dir());
    assert!(!expected.join(".git").exists());
    std::fs::write(expected.join("kept.md"), b"keep me").unwrap();

    let reregistered = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    assert_eq!(reregistered.notes_path, project.notes_path);
    assert_eq!(std::fs::read(expected.join("kept.md")).unwrap(), b"keep me");
}

#[tokio::test]
async fn registers_a_local_repository_without_a_remote_under_localhost() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repo");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    let manager = AowManager::persistent_with_notes_base(
        &directory.path().join("state"),
        directory.path().join("aow"),
    )
    .unwrap();

    let project = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();

    let expected = directory
        .path()
        .join("aow")
        .join("localhost")
        .join(notes::process_account_name().unwrap())
        .join("repo");
    assert_eq!(Path::new(&project.notes_path), expected);
    assert!(expected.is_dir());
    assert!(!expected.join(".git").exists());
}

#[tokio::test]
async fn allows_notes_directories_nested_in_another_repository() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("outer");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    let nested = repository.join("notes");
    let result = notes::prepare_notes_directory(&nested).await.unwrap();
    assert_eq!(Path::new(&result), nested);
    assert!(nested.is_dir());
    assert!(!nested.join(".git").exists());
}

#[test]
fn executable_resolution_does_not_invoke_the_command() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("agent");
    std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        resolve_executable("agent", &[directory.path().to_path_buf()]),
        Some(executable)
    );
}

#[test]
fn agent_discovery_only_registers_the_traecli_command() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [directory.path().to_path_buf()];
    let manager = AowManager::in_memory();
    for command in ["traex", "traecli"] {
        let executable = directory.path().join(command);
        std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let agents = manager.agents_in_path(&paths).unwrap();
        if command == "traex" {
            assert!(agents.is_empty());
        } else {
            assert_eq!(agents.len(), 1);
            assert_eq!(agents[0].id, "traecli");
            assert_eq!(agents[0].display_name, "TraeCode CLI");
            assert_eq!(agents[0].source, "detected");
            assert_eq!(agents[0].executable.as_deref(), executable.to_str());
        }
    }
}

#[test]
fn agent_discovery_is_limited_to_supported_registration_types() {
    let directory = tempfile::tempdir().unwrap();
    for agent in aow_agents::KNOWN_AGENTS {
        for command in agent.definition().commands {
            let executable = directory.path().join(command);
            std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let agents = AowManager::in_memory()
        .agents_in_path(&[directory.path().to_path_buf()])
        .unwrap();
    assert_eq!(agents.len(), AgentType::ALL.len());
    for agent_type in AgentType::ALL {
        assert!(
            agents.iter().any(|agent| {
                agent.id == agent_type.id() && agent.agent_type == Some(agent_type)
            })
        );
    }
}

#[tokio::test]
async fn agent_registration_api_requires_a_supported_type() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let directory = tempfile::tempdir().unwrap();
    let app = crate::build_router(AppState::new(directory.path().to_path_buf()));
    for agent_type in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!("")),
        Some(serde_json::json!("gemini")),
        Some(serde_json::json!("custom")),
    ] {
        let mut request =
            serde_json::json!({"display_name": "Custom wrapper", "command": "/bin/sh"});
        if let Some(agent_type) = agent_type {
            request["agent_type"] = agent_type;
        }
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/aow/agents")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "{request}: {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn agent_types_persist_for_multiple_custom_launch_configurations() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    manager
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![directory.path().to_path_buf()]),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut registrations = Vec::new();
    for agent_type in [
        AgentType::Claude,
        AgentType::Codex,
        AgentType::TraeCli,
        AgentType::Codex,
    ] {
        let registration = manager.register_agent(serde_json::from_value(serde_json::json!({
            "agent_type": agent_type, "display_name": "My wrapper", "command": "/bin/sh",
            "args": ["-c", "printf configured"], "env": {"CUSTOM_ENDPOINT": "value with spaces", "EMPTY": ""}
        })).unwrap()).await.unwrap();
        assert_eq!(registration.agent_type, Some(agent_type));
        assert!(registration.available);
        registrations.push(registration);
    }
    let manager = AowManager::persistent(directory.path()).unwrap();
    let agents = manager.agents().await.unwrap();
    assert_eq!(agents.len(), 4);
    assert_eq!(
        agents
            .iter()
            .map(|agent| &agent.id)
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    for registration in &registrations {
        let saved = agents
            .iter()
            .find(|agent| agent.id == registration.id)
            .unwrap();
        assert_eq!(saved.agent_type, registration.agent_type);
        let launch = saved
            .clone()
            .into_launch(&[directory.path().to_path_buf()])
            .unwrap();
        assert_eq!(Some(launch.agent_type), registration.agent_type);
        assert_eq!(launch.executable, "/bin/sh");
        assert_eq!(launch.args, ["-c", "printf configured"]);
        assert_eq!(launch.env["CUSTOM_ENDPOINT"], "value with spaces");
        assert_eq!(launch.env["EMPTY"], "");
    }
    let updated = manager.register_agent(serde_json::from_value(serde_json::json!({
        "id": registrations[0].id, "agent_type": "traecli", "display_name": "Another wrapper",
        "command": "/bin/sh", "args": ["--custom"]
    })).unwrap()).await.unwrap();
    assert_eq!(updated.agent_type, Some(AgentType::TraeCli));
    assert_eq!(manager.agents().await.unwrap().len(), 4);
}

#[tokio::test]
async fn legacy_agent_configuration_requires_type_only_when_identity_is_unknown() {
    let manager = AowManager::in_memory();
    for id in ["codex", "custom-wrapper"] {
        manager.lock().unwrap().agents.push(serde_json::from_value(serde_json::json!({
            "id": id, "display_name": id, "command": "/bin/sh", "args": ["--keep"], "env": {"KEEP": "value"}
        })).unwrap());
    }
    let agents = manager.agents_in_path(&[]).unwrap();
    let builtin = agents.iter().find(|agent| agent.id == "codex").unwrap();
    assert_eq!(builtin.agent_type, Some(AgentType::Codex));
    assert!(builtin.available);
    let legacy = agents
        .iter()
        .find(|agent| agent.id == "custom-wrapper")
        .unwrap();
    assert_eq!(legacy.agent_type, None);
    assert!(!legacy.available);
    assert_eq!(legacy.command, "/bin/sh");
    assert_eq!(legacy.args, ["--keep"]);
    assert_eq!(legacy.env["KEEP"], "value");
    assert!(matches!(
        legacy.clone().into_launch(&[]),
        Err(aow_agents::launch::LaunchError::Invalid(_))
    ));
    let mut update = serde_json::to_value(legacy).unwrap();
    update["agent_type"] = serde_json::json!("claude");
    let saved = manager
        .register_agent(serde_json::from_value(update).unwrap())
        .await
        .unwrap();
    assert_eq!(saved.id, "custom-wrapper");
    assert_eq!(saved.agent_type, Some(AgentType::Claude));
    assert_eq!(saved.args, legacy.args);
    assert_eq!(saved.env, legacy.env);
    assert!(saved.available);
    let mismatch = manager.register_agent(serde_json::from_value(serde_json::json!({
        "id": "codex", "agent_type": "claude", "display_name": "Mismatch", "command": "/bin/sh"
    })).unwrap()).await;
    assert!(matches!(mismatch, Err(AowError::Invalid(_))));
    assert_eq!(
        manager
            .agents_in_path(&[])
            .unwrap()
            .iter()
            .find(|agent| agent.id == "codex")
            .unwrap()
            .agent_type,
        Some(AgentType::Codex)
    );
}

#[tokio::test]
async fn detected_agent_configuration_persists_launches_and_resets() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("codex");
    std::fs::write(
        &executable,
        b"#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" \"$AOW_AGENT_TEST\" \"$EMPTY\" \"$HOME\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    manager
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![bin.clone()]),
            ..Default::default()
        })
        .await
        .unwrap();
    let detected = manager.agents().await.unwrap().pop().unwrap();
    assert_eq!(detected.id, "codex");
    assert_eq!(detected.source, "detected");
    assert_eq!(detected.command, "codex");
    let request = serde_json::json!({
        "id": detected.id,
        "agent_type": "codex",
        "display_name": detected.display_name,
        "command": detected.command,
        "args": ["--model", "model with spaces"],
        "env": {"AOW_AGENT_TEST": "value with spaces=equals", "EMPTY": "", "HOME": "/agent/home"}
    });
    manager
        .register_agent(serde_json::from_value(request.clone()).unwrap())
        .await
        .unwrap();

    let manager = AowManager::persistent(directory.path()).unwrap();
    let agents = manager.agents().await.unwrap();
    assert_eq!(
        agents.len(),
        1,
        "overrides must not duplicate detected agents"
    );
    let configured = &agents[0];
    assert_eq!(configured.id, "codex");
    assert_eq!(configured.source, "configured");
    assert_eq!(configured.command, "codex");
    assert_eq!(configured.args, ["--model", "model with spaces"]);
    assert_eq!(configured.env["EMPTY"], "");
    assert_eq!(
        std::fs::metadata(manager.inner.agents_path.as_ref().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let launch = agents
        .into_iter()
        .next()
        .unwrap()
        .into_launch(&[bin.clone()])
        .unwrap();
    let output = StdCommand::new(&launch.executable)
        .args(&launch.args)
        .env_clear()
        .envs(&launch.env)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "--model\nmodel with spaces\nvalue with spaces=equals\n\n/agent/home\n"
    );

    let mut update = request;
    update["args"] = serde_json::json!([]);
    update["env"] = serde_json::json!({});
    manager
        .register_agent(serde_json::from_value(update).unwrap())
        .await
        .unwrap();
    let updated = manager.agents().await.unwrap();
    assert_eq!(updated.len(), 1);
    assert!(updated[0].args.is_empty());
    assert!(updated[0].env.is_empty());
    manager.remove_agent("codex").unwrap();
    let restored = AowManager::persistent(directory.path())
        .unwrap()
        .agents()
        .await
        .unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].source, "detected");
    assert!(restored[0].args.is_empty());
    assert!(restored[0].env.is_empty());
}

#[tokio::test]
async fn agent_configuration_rejects_invalid_environment_and_failed_writes() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    let request = serde_json::json!({
        "id": "custom-test", "agent_type": "codex", "display_name": "Test", "command": "/bin/sh", "env_keys": ["HOME"]
    });
    let legacy: StoredAgent = serde_json::from_value(request.clone()).unwrap();
    assert!(legacy.env.is_empty());
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("env_keys")
            .is_none()
    );
    for env in [
        serde_json::json!({"BAD=KEY": "value"}),
        serde_json::json!({"KEY": "bad\0value"}),
    ] {
        let mut invalid = request.clone();
        invalid["env"] = env;
        assert!(matches!(
            manager
                .register_agent(serde_json::from_value(invalid).unwrap())
                .await,
            Err(AowError::Invalid(_))
        ));
        assert!(manager.lock().unwrap().agents.is_empty());
    }
    manager
        .register_agent(serde_json::from_value(request.clone()).unwrap())
        .await
        .unwrap();
    let registry = manager.inner.agents_path.as_ref().unwrap();
    std::fs::remove_file(&registry).unwrap();
    std::fs::create_dir(&registry).unwrap();
    let mut update = request.clone();
    update["args"] = serde_json::json!(["--changed"]);
    assert!(
        manager
            .register_agent(serde_json::from_value(update).unwrap())
            .await
            .is_err()
    );
    assert!(manager.lock().unwrap().agents[0].args.is_empty());
    let mut create = request;
    create["id"] = serde_json::json!("another-agent");
    assert!(
        manager
            .register_agent(serde_json::from_value(create).unwrap())
            .await
            .is_err()
    );
    assert_eq!(manager.lock().unwrap().agents.len(), 1);
}

#[test]
fn agent_launch_resolves_env_shebang_with_discovered_path() {
    let directory = tempfile::tempdir().unwrap();
    let cli_bin = directory.path().join("cli bin");
    let node_bin = directory.path().join("node bin");
    std::fs::create_dir(&cli_bin).unwrap();
    std::fs::create_dir(&node_bin).unwrap();
    let executable = cli_bin.join("codex");
    let node = node_bin.join("node");
    std::fs::write(&executable, b"#!/usr/bin/env node\n").unwrap();
    // A stand-in interpreter keeps the regression independent of installed Node.
    std::fs::write(&node, b"#!/bin/sh\nprintf 'node-ready:%s\\n' \"$2\"\n").unwrap();
    for path in [&executable, &node] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = vec![cli_bin.clone(), node_bin];
    let manager = AowManager::in_memory();

    // The CLI itself can be found even when its interpreter cannot.
    let missing_node = StdCommand::new(&executable)
        .env_clear()
        .env("PATH", &cli_bin)
        .output()
        .unwrap();
    assert_eq!(missing_node.status.code(), Some(127));

    for configured in [false, true] {
        if configured {
            manager.lock().unwrap().agents.push(StoredAgent {
                agent_type: Some(AgentType::Codex),
                id: "codex".to_owned(),
                display_name: "Configured Codex".to_owned(),
                command: executable.to_string_lossy().into_owned(),
                args: vec!["--version".to_owned()],
                env: BTreeMap::new(),
            });
        }
        let agent = manager
            .agents_in_path(&paths)
            .unwrap()
            .into_iter()
            .find(|agent| agent.id == "codex" && agent.available)
            .unwrap();
        assert_eq!(
            agent.source,
            if configured { "configured" } else { "detected" }
        );
        let launch = agent.into_launch(&paths).unwrap();
        let output = StdCommand::new(&launch.executable)
            .args(&launch.args)
            .env_clear()
            .env("PATH", &cli_bin)
            .envs(&launch.env)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "agent failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            if configured {
                "node-ready:--version\n"
            } else {
                "node-ready:\n"
            }
        );
    }
}

#[test]
fn agent_launch_adds_configured_environment_with_global_path() {
    let agent = AgentRegistration {
        id: "custom".to_owned(),
        agent_type: Some(AgentType::Codex),
        display_name: "Custom Agent".to_owned(),
        source: "configured",
        available: true,
        command: "/bin/sh".to_owned(),
        executable: Some("/bin/sh".to_owned()),
        args: Vec::new(),
        env: BTreeMap::from([
            ("PATH".to_owned(), "/agent/bin".to_owned()),
            ("AOW_AGENT_TEST".to_owned(), "value".to_owned()),
            ("EMPTY".to_owned(), String::new()),
        ]),
    };
    let paths = vec![PathBuf::from("/discovered/bin")];
    let launch = agent.into_launch(&paths).unwrap();
    assert_eq!(launch.env["PATH"], "/discovered/bin");
    assert_eq!(launch.env["AOW_AGENT_TEST"], "value");
    assert_eq!(launch.env["EMPTY"], "");
    assert_eq!(launch.env.len(), 3);
}

fn test_agent_session(
    agent: &'static str,
    session_id: &str,
    cwd: &Path,
    transcript_path: &Path,
    trusted_root: &Path,
) -> aow_agents::sessions::AgentSession {
    aow_agents::sessions::AgentSession::new(
        aow_agents::sessions::AgentSessionLocator {
            agent,
            session_id: session_id.into(),
            title: "Test session".into(),
            cwd: cwd.into(),
            transcript_path: transcript_path.into(),
            trusted_root: trusted_root.into(),
        },
        chrono::Utc::now(),
        chrono::Utc::now(),
    )
}

#[test]
fn session_locator_is_scoped_to_the_exact_registered_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("repo");
    let nested = parent.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let transcript = directory.path().join("sessions/session.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, b"").unwrap();
    let manager = AowManager::in_memory();
    let sessions = vec![
        test_agent_session(
            "traecli",
            "session-1",
            &nested,
            &transcript,
            transcript.parent().unwrap(),
        ),
        test_agent_session(
            "codex",
            "session-2",
            &nested,
            &transcript,
            transcript.parent().unwrap(),
        ),
    ];

    manager
        .replace_session_locators_for_agent(&parent, None, &sessions)
        .unwrap();
    assert!(
        manager
            .session_locator(&parent, "traecli", "session-1")
            .is_ok()
    );
    assert!(
        manager
            .session_locator(&nested, "traecli", "session-1")
            .is_err()
    );
    assert!(
        manager
            .session_locator(&parent, "codex", "session-1")
            .is_err()
    );

    manager
        .replace_session_locators_for_agent(&parent, Some("traecli"), &[])
        .unwrap();
    assert!(
        manager
            .session_locator(&parent, "traecli", "session-1")
            .is_err()
    );
    assert!(
        manager
            .session_locator(&parent, "codex", "session-2")
            .is_ok()
    );
}

#[test]
fn removed_session_worktree_paths_must_stay_absolute_and_normalized() {
    assert_eq!(
        sessions::removed_directory_path(Path::new("/tmp/removed-worktree")).unwrap(),
        Path::new("/tmp/removed-worktree")
    );
    assert!(sessions::removed_directory_path(Path::new("relative/worktree")).is_err());
    assert!(sessions::removed_directory_path(Path::new("/tmp/../removed-worktree")).is_err());
}

#[tokio::test]
async fn optionally_pulls_main_repository_when_registered_from_linked_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let origin = directory.path().join("origin");
    let repository = directory.path().join("repo");
    let linked = directory.path().join("repo-linked");
    git_output(
        directory.path(),
        &["init", "-q", "-b", "main", origin.to_str().unwrap()],
    )
    .await
    .unwrap();
    let commit_args = [
        "-c",
        "user.name=AoW Test",
        "-c",
        "user.email=aow@example.com",
        "commit",
        "-q",
        "-m",
        "update version",
    ];
    std::fs::write(origin.join("version.txt"), "initial\n").unwrap();
    git_output(&origin, &["add", "version.txt"]).await.unwrap();
    git_output(&origin, &commit_args).await.unwrap();
    git_output(
        directory.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            repository.to_str().unwrap(),
        ],
    )
    .await
    .unwrap();
    git_output(
        &repository,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await
    .unwrap();
    let initial_head = git_output(&repository, &["rev-parse", "HEAD"])
        .await
        .unwrap();
    std::fs::write(origin.join("version.txt"), "latest\n").unwrap();
    git_output(&origin, &["add", "version.txt"]).await.unwrap();
    git_output(&origin, &commit_args).await.unwrap();
    let latest_head = git_output(&origin, &["rev-parse", "HEAD"]).await.unwrap();
    assert_ne!(initial_head, latest_head);

    let manager = AowManager::in_memory();
    let project = manager
        .register_project(RegisterProjectRequest {
            path: linked.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(
                directory
                    .path()
                    .join("notes")
                    .to_string_lossy()
                    .into_owned(),
            ),
        })
        .await
        .unwrap();

    for pull_first in [false, true] {
        let worktree_path = directory.path().join(format!("repo-pull-{pull_first}"));
        let result = manager
            .create_worktree(
                &project.id,
                CreateWorktreeRequest {
                    branch: format!("feature/pull-{pull_first}"),
                    base_ref: "main".to_owned(),
                    path: worktree_path.to_string_lossy().into_owned(),
                    pull_first,
                },
            )
            .await
            .unwrap();
        let expected_head = if pull_first {
            &latest_head
        } else {
            &initial_head
        };
        assert_eq!(result.worktree.head, expected_head.trim());
        assert_eq!(
            git_output(&repository, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            *expected_head
        );
        assert_eq!(
            std::fs::read_to_string(worktree_path.join("version.txt")).unwrap(),
            if pull_first { "latest\n" } else { "initial\n" }
        );
        assert_eq!(
            git_output(&linked, &["rev-parse", "HEAD"]).await.unwrap(),
            initial_head
        );
    }
}

#[tokio::test]
async fn creates_worktree_and_returns_refreshed_project() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = root.join("repo");
    assert!(
        StdCommand::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args([
                "-c",
                "user.name=AoW Test",
                "-c",
                "user.email=aow@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "initial",
            ])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );

    let state_directory = root.join("state");
    let manager = AowManager::persistent(&state_directory).unwrap();
    let project = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(root.join("notes").to_string_lossy().into_owned()),
        })
        .await
        .unwrap();
    let worktree_path = root.join("repo-feature");
    let failed_pull = manager
        .create_worktree(
            &project.id,
            CreateWorktreeRequest {
                branch: "feature/aow".to_owned(),
                base_ref: "main".to_owned(),
                path: worktree_path.to_string_lossy().into_owned(),
                pull_first: true,
            },
        )
        .await
        .unwrap_err();
    assert!(
        failed_pull
            .to_string()
            .contains("git pull failed in main worktree")
    );
    assert!(!worktree_path.exists());
    assert!(
        git_output(&repository, &["branch", "--list", "feature/aow"])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        manager.project(&project.id).await.unwrap().worktrees.len(),
        1
    );

    let result = manager
        .create_worktree(
            &project.id,
            CreateWorktreeRequest {
                branch: "feature/aow".to_owned(),
                base_ref: "main".to_owned(),
                path: worktree_path.to_string_lossy().into_owned(),
                pull_first: false,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.worktree.branch, "feature/aow");
    assert_eq!(
        Path::new(&result.worktree.path).canonicalize().unwrap(),
        worktree_path.canonicalize().unwrap()
    );
    assert_eq!(
        manager
            .project_name_for_workspace(repository.to_str().unwrap())
            .await,
        Some(project.name.clone())
    );
    assert_eq!(
        manager
            .project_name_for_workspace(worktree_path.to_str().unwrap())
            .await,
        Some(project.name.clone())
    );
    assert_eq!(result.project.worktrees.len(), 2);
    assert!(
        result
            .project
            .worktrees
            .iter()
            .any(|worktree| worktree.id == result.worktree.id)
    );
    assert_eq!(result.worktree.color, WorktreeColor::Default);
    assert_eq!(result.worktree.icon, WorktreeIcon::Default);

    let icon_request = |icon| SetWorktreeIconRequest {
        path: worktree_path.to_string_lossy().into_owned(),
        icon,
    };
    manager
        .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Cat))
        .await
        .unwrap();

    let colored = manager
        .set_worktree_color(
            &project.id,
            SetWorktreeColorRequest {
                path: worktree_path.to_string_lossy().into_owned(),
                color: WorktreeColor::Purple,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        colored
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == worktree_path)
            .unwrap()
            .icon,
        WorktreeIcon::Cat
    );
    assert_eq!(
        colored
            .worktrees
            .iter()
            .find(|worktree| Path::new(&worktree.path) == worktree_path)
            .unwrap()
            .color,
        WorktreeColor::Purple
    );
    drop(manager);
    let manager = AowManager::persistent(&state_directory).unwrap();
    let reloaded = manager.project(&project.id).await.unwrap();
    assert_eq!(
        reloaded
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == worktree_path)
            .unwrap()
            .icon,
        WorktreeIcon::Cat
    );
    assert_eq!(
        reloaded
            .worktrees
            .iter()
            .find(|worktree| Path::new(&worktree.path) == worktree_path)
            .unwrap()
            .color,
        WorktreeColor::Purple
    );

    // Changing or resetting the shape preserves the independently selected color.
    for icon in [
        WorktreeIcon::Pear,
        WorktreeIcon::Default,
        WorktreeIcon::Flower2,
    ] {
        let updated = manager
            .set_worktree_icon(&project.id, icon_request(icon))
            .await
            .unwrap();
        let worktree = updated
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == worktree_path)
            .unwrap();
        assert_eq!(worktree.icon, icon);
        assert_eq!(worktree.color, WorktreeColor::Purple);
        if icon == WorktreeIcon::Default {
            assert!(
                !manager
                    .lock()
                    .unwrap()
                    .projects
                    .iter()
                    .find(|p| p.id == project.id)
                    .unwrap()
                    .worktree_icons
                    .contains_key(worktree_path.to_str().unwrap())
            );
        }
    }
    assert!(matches!(
        manager
            .set_worktree_icon(
                &project.id,
                SetWorktreeIconRequest {
                    path: root.join("not-a-worktree").to_string_lossy().into_owned(),
                    icon: WorktreeIcon::Cat,
                }
            )
            .await,
        Err(AowError::Invalid(_))
    ));

    // A failed disk write must restore the in-memory shape as well.
    let projects_path = manager.inner.projects_path.as_ref().unwrap();
    let saved_projects = std::fs::read(projects_path).unwrap();
    std::fs::remove_file(projects_path).unwrap();
    std::fs::create_dir(projects_path).unwrap();
    assert!(
        manager
            .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Dog))
            .await
            .is_err()
    );
    let unchanged = manager.project(&project.id).await.unwrap();
    let worktree = unchanged
        .worktrees
        .iter()
        .find(|w| Path::new(&w.path) == worktree_path)
        .unwrap();
    assert_eq!(worktree.icon, WorktreeIcon::Flower2);
    assert_eq!(worktree.color, WorktreeColor::Purple);
    std::fs::remove_dir(projects_path).unwrap();
    std::fs::write(projects_path, saved_projects).unwrap();

    let duplicate_path = root.join("repo-duplicate");
    let duplicate = manager
        .create_worktree(
            &project.id,
            CreateWorktreeRequest {
                branch: "feature/aow".to_owned(),
                base_ref: "main".to_owned(),
                path: duplicate_path.to_string_lossy().into_owned(),
                pull_first: false,
            },
        )
        .await;
    assert!(matches!(duplicate, Err(AowError::Git(_))));
    assert!(!duplicate_path.exists());

    let existing_path = root.join("already-exists");
    std::fs::create_dir(&existing_path).unwrap();
    let existing = manager
        .create_worktree(
            &project.id,
            CreateWorktreeRequest {
                branch: "feature/other".to_owned(),
                base_ref: "main".to_owned(),
                path: existing_path.to_string_lossy().into_owned(),
                pull_first: false,
            },
        )
        .await;
    assert!(matches!(existing, Err(AowError::Invalid(_))));

    std::fs::write(worktree_path.join("uncommitted.txt"), b"not committed\n").unwrap();
    let inspection = manager
        .inspect_worktree_removal(&project.id, worktree_path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(inspection.changes, vec!["?? uncommitted.txt"]);
    assert_eq!(inspection.change_count, 1);
    assert!(!inspection.truncated);

    let without_force = manager
        .remove_worktree(&project.id, worktree_path.to_str().unwrap(), false)
        .await;
    assert!(matches!(without_force, Err(AowError::DirtyWorktree(1))));
    assert!(worktree_path.exists());

    let reregistered = manager
        .register_project(RegisterProjectRequest {
            path: worktree_path.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    assert_eq!(reregistered.id, project.id);
    assert_eq!(Path::new(&reregistered.registered_path), worktree_path);

    let refreshed = manager
        .remove_worktree(&project.id, worktree_path.to_str().unwrap(), true)
        .await
        .unwrap();
    assert!(!worktree_path.exists());
    let document: RegistryDocument<StoredProject> = serde_json::from_slice(
        &std::fs::read(manager.inner.projects_path.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        !document.items[0]
            .worktree_colors
            .contains_key(worktree_path.to_str().unwrap())
    );
    assert!(
        !document.items[0]
            .worktree_icons
            .contains_key(worktree_path.to_str().unwrap())
    );
    assert_eq!(Path::new(&refreshed.registered_path), repository);
    assert_eq!(refreshed.worktrees.len(), 1);
    assert!(refreshed.worktrees[0].is_main);
    let branches = git_output(&repository, &["branch", "--format=%(refname:short)"])
        .await
        .unwrap();
    assert!(branches.lines().any(|branch| branch == "feature/aow"));

    let clean_path = root.join("repo-clean");
    manager
        .create_worktree(
            &project.id,
            CreateWorktreeRequest {
                branch: "feature/clean".to_owned(),
                base_ref: "main".to_owned(),
                path: clean_path.to_string_lossy().into_owned(),
                pull_first: false,
            },
        )
        .await
        .unwrap();
    let clean_inspection = manager
        .inspect_worktree_removal(&project.id, clean_path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(clean_inspection.change_count, 0);
    manager
        .remove_worktree(&project.id, clean_path.to_str().unwrap(), false)
        .await
        .unwrap();
    assert!(!clean_path.exists());

    let main_removal = manager
        .inspect_worktree_removal(&project.id, repository.to_str().unwrap())
        .await;
    assert!(matches!(main_removal, Err(AowError::Invalid(_))));
}
