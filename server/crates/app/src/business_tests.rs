use super::*;
use crate::agents::{StoredAgent, sessions};
use crate::error::ServiceError;
use crate::projects::worktrees::{CreateWorktreeRequest, WorktreeColor, WorktreeIcon};
use crate::projects::{RegisterProjectRequest, notes};
use crate::projects::{
    StoredProject,
    worktrees::{self, SetWorktreeColorRequest, SetWorktreeIconRequest},
};
use crate::settings::persistence::RegistryDocument;
use crate::settings::{EditorSettings, UpdateSettingsRequest};
use crate::settings::{UpdatePinnedDirectoriesRequest, UpdatePinnedWorktreesRequest};
use crate::test_support::{TestServices, git_output};
use aow_config::environment::SETTINGS_FILE;
use aow_runtime_contract::agents::AgentRegistration;
use aow_runtime_contract::agents::AgentType;
use axum::http::StatusCode;
use std::path::{Path, PathBuf};

use std::{
    collections::{BTreeMap, HashSet},
    os::unix::fs::PermissionsExt,
    process::Command as StdCommand,
};

#[tokio::test]
async fn prepared_worktree_uses_selected_base_and_lists_unchecked_local_and_remote_branches() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_output(&repo, &["init", "-q", "-b", "main"])
        .await
        .unwrap();
    git_output(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .await
    .unwrap();
    git_output(&repo, &["branch", "feature/unopened"])
        .await
        .unwrap();
    let state = AppState::for_test(root.join("state"));
    let manager = state.clone();
    let project = manager
        .projects
        .register_project(RegisterProjectRequest {
            path: repo.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(root.join("notes").to_string_lossy().into_owned()),
        })
        .await
        .unwrap();
    let local = manager
        .projects
        .project_branches(&project.id)
        .await
        .unwrap();
    assert_eq!(local.branches, ["feature/unopened", "main"]);
    assert_eq!(local.default_branch, "main");
    assert_eq!(project.worktrees.len(), 1);

    let main_head = git_output(&repo, &["rev-parse", "HEAD"]).await.unwrap();
    let release_head = git_output(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit-tree",
            "HEAD^{tree}",
            "-p",
            "HEAD",
            "-m",
            "release",
        ],
    )
    .await
    .unwrap();
    git_output(
        &repo,
        &["update-ref", "refs/remotes/origin/main", main_head.trim()],
    )
    .await
    .unwrap();
    git_output(
        &repo,
        &[
            "update-ref",
            "refs/remotes/origin/release",
            release_head.trim(),
        ],
    )
    .await
    .unwrap();
    git_output(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await
    .unwrap();

    let app = crate::build_router(state);
    let response = app
        .oneshot(
            Request::get(format!("/api/projects/{}/branches", project.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let branches: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        branches,
        serde_json::json!({
            "branches": ["feature/unopened", "main", "origin/main", "origin/release"],
            "default_branch": "origin/main",
        })
    );
}

#[tokio::test]
async fn automation_cwd_accepts_worktree_subdirectories_but_rejects_other_repositories() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_output(&repo, &["init", "-q", "-b", "main"])
        .await
        .unwrap();
    git_output(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .await
    .unwrap();
    let linked = directory.path().join("linked");
    git_output(
        &repo,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await
    .unwrap();
    let manager = TestServices::persistent_with_notes_base(
        &directory.path().join("state"),
        directory.path().join("notes"),
    )
    .unwrap();
    let project = manager
        .projects
        .register_project(RegisterProjectRequest {
            path: repo.to_string_lossy().into(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    for root in [&repo, &linked] {
        let cwd = root.join("frontend/src");
        std::fs::create_dir_all(&cwd).unwrap();
        for allowed in [root, &cwd] {
            let (_, registered) = manager
                .projects
                .automation_project_for_cwd(&project.id, allowed)
                .await
                .unwrap();
            assert_eq!(registered, repo.canonicalize().unwrap());
        }
    }
    for foreign in [
        repo.join("nested-repository"),
        directory.path().join("other"),
    ] {
        std::fs::create_dir(&foreign).unwrap();
        git_output(&foreign, &["init", "-q"]).await.unwrap();
        let error = manager
            .projects
            .automation_project_for_cwd(&project.id, &foreign)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Workspace does not belong to the selected project"),
            "{error}"
        );
    }
    let outside_link = repo.join("linked-outside");
    std::os::unix::fs::symlink(directory.path().join("other"), &outside_link).unwrap();
    assert!(
        manager
            .projects
            .automation_project_for_cwd(&project.id, &outside_link)
            .await
            .is_err()
    );
}

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
    let manager = TestServices::persistent(directory.path()).unwrap();
    assert!(
        manager
            .settings
            .settings()
            .unwrap()
            .node_addresses
            .is_empty()
    );
    let update = serde_json::from_value(serde_json::json!({
        "node_addresses": [" HTTPS://NODE-A.EXAMPLE:443 ", "", "https://node-a.example/",
            "http://192.168.1.2:8080/aow/?ui=desktop#tab", "http://[::1]:8080"]
    }))
    .unwrap();
    let saved = manager.settings.update_settings(update).await.unwrap();
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
        .settings
        .update_settings(UpdateSettingsRequest {
            editor: Some(EditorSettings { word_wrap: false }),
            ..Default::default()
        })
        .await
        .unwrap();
    let reloaded = TestServices::persistent(directory.path()).unwrap();
    assert_eq!(
        reloaded.settings.settings().unwrap().node_addresses,
        expected
    );
    reloaded
        .settings
        .update_settings(UpdateSettingsRequest {
            node_addresses: Some(Vec::new()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        TestServices::persistent(directory.path())
            .unwrap()
            .settings
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
        TestServices::persistent_with_notes_base(directory.path(), directory.path().join("notes"))
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
                .settings
                .update_settings(UpdateSettingsRequest {
                    node_addresses: Some(vec!["https://valid.example".into(), address.into()]),
                    ..Default::default()
                })
                .await
                .is_err(),
            "accepted invalid address: {address}"
        );
        assert!(
            manager
                .settings
                .settings()
                .unwrap()
                .node_addresses
                .is_empty()
        );
    }
    std::fs::create_dir(manager.settings.test_settings_path().unwrap()).unwrap();
    assert!(
        manager
            .settings
            .update_settings(UpdateSettingsRequest {
                node_addresses: Some(vec!["https://node.example".into()]),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(
        manager
            .settings
            .settings()
            .unwrap()
            .node_addresses
            .is_empty()
    );
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    assert!(!manager.settings.settings().unwrap().editor.word_wrap);
    let update = serde_json::from_value(serde_json::json!({
        "editor": { "word_wrap": true }
    }))
    .unwrap();
    let saved = manager.settings.update_settings(update).await.unwrap();
    assert!(saved.editor.word_wrap);
    assert_eq!(Path::new(&saved.notes_base), notes_base);
    assert_eq!(
        saved.execution_path.unwrap(),
        vec![PathBuf::from("/usr/bin")]
    );
    assert_eq!(saved.pinned_worktrees, ["/repo/main"]);
    assert_eq!(saved.pinned_worktrees_revision, 7);

    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![PathBuf::from("/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    let reloaded = TestServices::persistent(directory.path()).unwrap();
    assert!(reloaded.settings.settings().unwrap().editor.word_wrap);
    assert_eq!(
        reloaded.settings.execution_path().await.unwrap(),
        vec![PathBuf::from("/bin")]
    );
    reloaded
        .settings
        .update_settings(UpdateSettingsRequest {
            editor: Some(EditorSettings { word_wrap: false }),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        !TestServices::persistent(directory.path())
            .unwrap()
            .settings
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
        TestServices::persistent_with_notes_base(directory.path(), directory.path().join("notes"))
            .unwrap();
    std::fs::create_dir(manager.settings.test_settings_path().unwrap()).unwrap();
    assert!(
        manager
            .settings
            .update_settings(UpdateSettingsRequest {
                editor: Some(EditorSettings { word_wrap: true }),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(!manager.settings.settings().unwrap().editor.word_wrap);
}

#[tokio::test]
async fn execution_path_defaults_persist_and_settings_updates_preserve_other_fields() {
    let directory = tempfile::tempdir().unwrap();
    let manager = TestServices::persistent_with_notes_base(
        directory.path(),
        directory.path().join("original notes"),
    )
    .unwrap();
    let initial = manager.settings.execution_path().await.unwrap();
    assert!(!initial.is_empty());
    assert_eq!(manager.settings.execution_path().await.unwrap(), initial);
    let bin = directory.path().join("custom bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("codex"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(bin.join("codex"), std::fs::Permissions::from_mode(0o700)).unwrap();
    manager
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/main".into()],
            ..Default::default()
        })
        .unwrap();
    let paths = vec![bin.clone(), PathBuf::from("/usr/bin")];
    let settings = manager
        .settings
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
            .agents
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
        .settings
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
    let reloaded = TestServices::persistent(directory.path()).unwrap();
    assert_eq!(reloaded.settings.execution_path().await.unwrap(), paths);
    assert_eq!(manager.settings.execution_path().await.unwrap(), paths);
    assert_eq!(
        reloaded.settings.pinned_worktrees().unwrap().paths,
        ["/repo/main"]
    );
    for invalid in [
        vec![],
        vec!["relative/bin".into()],
        vec!["/bin:/usr/bin".into()],
        vec![bin.join("codex")],
    ] {
        assert!(
            manager
                .settings
                .update_settings(UpdateSettingsRequest {
                    execution_path: Some(invalid),
                    ..Default::default()
                })
                .await
                .is_err()
        );
        assert_eq!(manager.settings.execution_path().await.unwrap(), paths);
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    assert!(
        manager
            .settings
            .pinned_directories()
            .unwrap()
            .paths
            .is_empty()
    );
    let first = directory.path().join("目录 #1");
    let second = directory.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let first = first.to_string_lossy().into_owned();
    let second = second.to_string_lossy().into_owned();
    let (one, two) = tokio::join!(
        manager
            .settings
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec![first.clone(), format!("{first}/")],
                ..Default::default()
            }),
        manager
            .settings
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                add: vec![second.clone()],
                ..Default::default()
            }),
    );
    one.unwrap();
    two.unwrap();
    let pins = manager.settings.pinned_directories().unwrap();
    assert_eq!(pins.paths.len(), 2);
    assert!(pins.paths.contains(&first));
    assert!(pins.paths.contains(&second));
    assert_eq!(pins.revision, 2);
    let duplicate = manager
        .settings
        .update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec![first.clone()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(duplicate.revision, pins.revision);
    let configured_notes = directory.path().join("configured-notes");
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(configured_notes.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    manager
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/feature".into()],
            ..Default::default()
        })
        .unwrap();
    drop(manager);
    let reloaded = TestServices::persistent(directory.path()).unwrap();
    assert_eq!(
        reloaded.settings.pinned_directories().unwrap().paths,
        pins.paths
    );
    assert_eq!(
        reloaded.settings.pinned_directories().unwrap().revision,
        pins.revision
    );
    assert_eq!(
        reloaded.settings.pinned_worktrees().unwrap().paths,
        ["/repo/main", "/repo/feature"]
    );
    assert_eq!(
        Path::new(&reloaded.settings.settings().unwrap().notes_base),
        configured_notes
    );
    std::fs::remove_dir(&first).unwrap();
    let remaining = reloaded
        .settings
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
        TestServices::persistent(directory.path())
            .unwrap()
            .settings
            .pinned_directories()
            .unwrap()
            .paths,
        remaining.paths
    );
}

#[tokio::test]
async fn pinned_directories_roll_back_when_config_write_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = TestServices::persistent(directory.path()).unwrap();
    let previous = manager
        .settings
        .update_pinned_directories(UpdatePinnedDirectoriesRequest {
            add: vec!["/".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let settings_path = manager.settings.test_settings_path().unwrap();
    std::fs::remove_file(settings_path).unwrap();
    std::fs::create_dir(settings_path).unwrap();
    assert!(
        manager
            .settings
            .update_pinned_directories(UpdatePinnedDirectoriesRequest {
                remove: vec!["/".into()],
                ..Default::default()
            })
            .await
            .is_err()
    );
    let unchanged = manager.settings.pinned_directories().unwrap();
    assert_eq!(unchanged.paths, previous.paths);
    assert_eq!(unchanged.revision, previous.revision);
}

#[tokio::test]
async fn pinned_directories_routes_validate_paths_and_reject_files() {
    use axum::http::StatusCode;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let directory = tempfile::tempdir().unwrap();
    let regular_file = directory.path().join("file.txt");
    std::fs::write(&regular_file, "not a directory").unwrap();
    let app = crate::build_router(AppState::for_test(directory.path().to_path_buf()));
    for invalid in [
        "relative/path",
        "/bad\0path",
        regular_file.to_str().unwrap(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::patch("/api/pinned-directories")
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
                Request::patch("/api/pinned-directories")
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
                Request::get("/api/pinned-directories")
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    assert!(
        manager
            .settings
            .pinned_worktrees()
            .unwrap()
            .paths
            .is_empty()
    );
    let pins = manager
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/b".into(), "/repo/a".into(), "/repo/b".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pins.paths, ["/repo/b", "/repo/a"]);
    manager
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/c".into()],
            ..Default::default()
        })
        .unwrap();
    let pins = manager
        .settings
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
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            remove: vec!["/repo/b".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pins.paths, ["/repo/a", "/repo/c"]);
    let configured_notes = directory.path().join("configured-notes");
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(configured_notes.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    drop(manager);
    let reloaded = TestServices::persistent(directory.path()).unwrap();
    assert_eq!(
        reloaded.settings.pinned_worktrees().unwrap().paths,
        pins.paths
    );
    assert_eq!(
        reloaded.settings.pinned_worktrees().unwrap().revision,
        pins.revision
    );
    assert_eq!(
        Path::new(&reloaded.settings.settings().unwrap().notes_base),
        configured_notes
    );
    assert!(
        reloaded
            .settings
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    let previous = manager
        .settings
        .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
            add: vec!["/repo/a".into()],
            ..Default::default()
        })
        .unwrap();
    let settings_path = manager.settings.test_settings_path().unwrap();
    std::fs::remove_file(settings_path).unwrap();
    std::fs::create_dir(settings_path).unwrap();
    assert!(
        manager
            .settings
            .update_pinned_worktrees(UpdatePinnedWorktreesRequest {
                add: vec!["/repo/b".into()],
                ..Default::default()
            })
            .is_err()
    );
    let unchanged = manager.settings.pinned_worktrees().unwrap();
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
    let manager = TestServices::persistent_with_notes_base(
        &state_directory,
        directory.path().join("default-notes"),
    )
    .unwrap();

    let settings = manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(notes_base.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(Path::new(&settings.notes_base), notes_base);
    assert!(notes_base.is_dir());

    let project = manager
        .projects
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
    let reloaded = TestServices::persistent_with_notes_base(
        &state_directory,
        directory.path().join("another-default"),
    )
    .unwrap();
    assert_eq!(
        Path::new(&reloaded.settings.settings().unwrap().notes_base),
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

#[tokio::test]
async fn temporary_notes_use_unique_node_paths() {
    let directory = tempfile::tempdir().unwrap();
    let manager = TestServices::in_memory();
    let (first, _) = manager
        .node
        .node_temporary_note(directory.path(), "md")
        .await
        .unwrap();
    let (second, _) = manager
        .node
        .node_temporary_note(directory.path(), "md")
        .await
        .unwrap();
    assert_ne!(first, second);
    assert!(first.is_file() && second.is_file());
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
    let manager = TestServices::persistent_with_notes_base(
        &directory.path().join("state"),
        directory.path().join("aow"),
    )
    .unwrap();
    let project = manager
        .projects
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
        .projects
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
    let manager = TestServices::persistent_with_notes_base(
        &directory.path().join("state"),
        directory.path().join("aow"),
    )
    .unwrap();

    let project = manager
        .projects
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
        .join(TestServices::in_memory().node.node_account().await.unwrap())
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
    let result = TestServices::in_memory()
        .node
        .prepare_notes_directory(&nested)
        .await
        .unwrap();
    assert_eq!(Path::new(&result), nested);
    assert!(nested.is_dir());
    assert!(!nested.join(".git").exists());
}

#[tokio::test]
async fn executable_resolution_does_not_invoke_the_command() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("agent");
    std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = TestServices::in_memory();
    manager
        .agents
        .agents_in_path(&[directory.path().into()])
        .await
        .unwrap();
    let launch = manager
        .node
        .agent_node()
        .await
        .unwrap()
        .resolve_agent(&aow_runtime_client::agents::ResolveAgent {
            profile: aow_runtime_client::agents::AgentProfile {
                id: "test".into(),
                agent_type: Some(AgentType::Codex),
                display_name: "Test".into(),
                command: "agent".into(),
                args: vec![],
                env: Default::default(),
            },
        })
        .await
        .unwrap();
    assert_eq!(Path::new(&launch.executable), executable);
}

#[tokio::test]
async fn agent_discovery_only_registers_the_traecli_command() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [directory.path().to_path_buf()];
    let manager = TestServices::in_memory();
    for command in ["traex", "traecli"] {
        let executable = directory.path().join(command);
        std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let agents = manager.agents.agents_in_path(&paths).await.unwrap();
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

#[tokio::test]
async fn agent_discovery_is_limited_to_supported_registration_types() {
    let directory = tempfile::tempdir().unwrap();
    for agent in TestServices::in_memory()
        .node
        .agent_node()
        .await
        .unwrap()
        .agent_capabilities()
        .await
        .unwrap()
    {
        for command in agent.commands {
            let executable = directory.path().join(command);
            std::fs::write(&executable, b"#!/bin/sh\nexit 99\n").unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let agents = TestServices::in_memory()
        .agents
        .agents_in_path(&[directory.path().to_path_buf()])
        .await
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
    let app = crate::build_router(AppState::for_test(directory.path().to_path_buf()));
    for agent_type in [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!("")),
        Some(serde_json::json!("gemini")),
        Some(serde_json::json!("custom")),
    ] {
        let mut request = serde_json::json!({"id": "custom-wrapper", "display_name": "Custom wrapper", "command": "/bin/sh"});
        if let Some(agent_type) = agent_type {
            request["agent_type"] = agent_type;
        }
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/agents")
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    manager
        .settings
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
        let registration = manager.agents.register_agent(serde_json::from_value(serde_json::json!({
            "id": format!("wrapper-{}", registrations.len()), "agent_type": agent_type, "display_name": "My wrapper", "command": "/bin/sh",
            "args": ["-c", "printf configured"], "env": {"CUSTOM_ENDPOINT": "value with spaces", "EMPTY": ""}
        })).unwrap()).await.unwrap();
        assert_eq!(registration.agent_type, Some(agent_type));
        assert!(registration.available);
        registrations.push(registration);
    }
    let manager = TestServices::persistent(directory.path()).unwrap();
    let agents = manager.agents.agents().await.unwrap();
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
        let launch = agents::resolve_on_node(saved.clone(), &[directory.path().to_path_buf()])
            .await
            .unwrap();
        assert_eq!(Some(launch.agent_type), registration.agent_type);
        assert_eq!(
            Path::new(&launch.executable),
            Path::new("/bin/sh").canonicalize().unwrap()
        );
        assert_eq!(launch.args, ["-c", "printf configured"]);
        assert_eq!(launch.env["CUSTOM_ENDPOINT"], "value with spaces");
        assert_eq!(launch.env["EMPTY"], "");
    }
    let updated = manager.agents.save_agent(Some(&registrations[0].id), serde_json::from_value(serde_json::json!({
        "id": registrations[0].id, "agent_type": "traecli", "display_name": "Another wrapper",
        "command": "/bin/sh", "args": ["--custom"]
    })).unwrap()).await.unwrap();
    assert_eq!(updated.agent_type, Some(AgentType::TraeCli));
    assert_eq!(manager.agents.agents().await.unwrap().len(), 4);
}

#[tokio::test]
async fn legacy_agent_configuration_requires_type_only_when_identity_is_unknown() {
    let manager = TestServices::in_memory();
    for id in ["codex", "custom-wrapper"] {
        manager.agents.test_registry().unwrap().agents.push(serde_json::from_value(serde_json::json!({
            "id": id, "display_name": id, "command": "/bin/sh", "args": ["--keep"], "env": {"KEEP": "value"}
        })).unwrap());
    }
    let agents = manager.agents.agents_in_path(&[]).await.unwrap();
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
        agents::resolve_on_node(legacy.clone(), &[]).await,
        Err(ServiceError::Invalid(_))
    ));
    let mut update = serde_json::to_value(legacy).unwrap();
    update["agent_type"] = serde_json::json!("claude");
    let saved = manager
        .agents
        .save_agent(
            Some(update["id"].as_str().unwrap()),
            serde_json::from_value(update.clone()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(saved.id, "custom-wrapper");
    assert_eq!(saved.agent_type, Some(AgentType::Claude));
    assert_eq!(saved.args, legacy.args);
    assert_eq!(saved.env, legacy.env);
    assert!(saved.available);
    let mismatch = manager.agents.save_agent(Some("codex"), serde_json::from_value(serde_json::json!({
        "id": "codex", "agent_type": "claude", "display_name": "Mismatch", "command": "/bin/sh"
    })).unwrap()).await;
    assert!(matches!(mismatch, Err(ServiceError::Invalid(_))));
    assert_eq!(
        manager
            .agents
            .agents_in_path(&[])
            .await
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
    let manager = TestServices::persistent(directory.path()).unwrap();
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            execution_path: Some(vec![bin.clone()]),
            ..Default::default()
        })
        .await
        .unwrap();
    let detected = manager.agents.agents().await.unwrap().pop().unwrap();
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
        .agents
        .save_agent(
            Some("codex"),
            serde_json::from_value(request.clone()).unwrap(),
        )
        .await
        .unwrap();

    let manager = TestServices::persistent(directory.path()).unwrap();
    let agents = manager.agents.agents().await.unwrap();
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
        std::fs::metadata(manager.agents.test_registry_path().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let launch = agents::resolve_on_node(
        agents.into_iter().next().unwrap(),
        std::slice::from_ref(&bin),
    )
    .await
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
        .agents
        .save_agent(
            Some(update["id"].as_str().unwrap()),
            serde_json::from_value(update.clone()).unwrap(),
        )
        .await
        .unwrap();
    let updated = manager.agents.agents().await.unwrap();
    assert_eq!(updated.len(), 1);
    assert!(updated[0].args.is_empty());
    assert!(updated[0].env.is_empty());
    manager.agents.remove_agent("codex").await.unwrap();
    let restored = TestServices::persistent(directory.path())
        .unwrap()
        .agents
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
    let manager = TestServices::persistent(directory.path()).unwrap();
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
                .agents
                .register_agent(serde_json::from_value(invalid).unwrap())
                .await,
            Err(ServiceError::Invalid(_))
        ));
        assert!(manager.agents.test_registry().unwrap().agents.is_empty());
    }
    manager
        .agents
        .register_agent(serde_json::from_value(request.clone()).unwrap())
        .await
        .unwrap();
    let registry = manager.agents.test_registry_path().unwrap();
    std::fs::remove_file(registry).unwrap();
    std::fs::create_dir(registry).unwrap();
    let mut update = request.clone();
    update["args"] = serde_json::json!(["--changed"]);
    assert!(
        manager
            .agents
            .save_agent(
                Some(update["id"].as_str().unwrap()),
                serde_json::from_value(update.clone()).unwrap(),
            )
            .await
            .is_err()
    );
    assert!(
        manager.agents.test_registry().unwrap().agents[0]
            .args
            .is_empty()
    );
    let mut create = request;
    create["id"] = serde_json::json!("another-agent");
    assert!(
        manager
            .agents
            .register_agent(serde_json::from_value(create).unwrap())
            .await
            .is_err()
    );
    assert_eq!(manager.agents.test_registry().unwrap().agents.len(), 1);
}

#[tokio::test]
async fn agent_launch_resolves_env_shebang_with_discovered_path() {
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
    let manager = TestServices::in_memory();

    // The CLI itself can be found even when its interpreter cannot.
    let missing_node = StdCommand::new(&executable)
        .env_clear()
        .env("PATH", &cli_bin)
        .output()
        .unwrap();
    assert_eq!(missing_node.status.code(), Some(127));

    for configured in [false, true] {
        if configured {
            manager
                .agents
                .test_registry()
                .unwrap()
                .agents
                .push(StoredAgent {
                    agent_type: Some(AgentType::Codex),
                    id: "codex".to_owned(),
                    display_name: "Configured Codex".to_owned(),
                    command: executable.to_string_lossy().into_owned(),
                    args: vec!["--version".to_owned()],
                    env: BTreeMap::new(),
                });
        }
        let agent = manager
            .agents
            .agents_in_path(&paths)
            .await
            .unwrap()
            .into_iter()
            .find(|agent| agent.id == "codex" && agent.available)
            .unwrap();
        assert_eq!(
            agent.source,
            if configured { "configured" } else { "detected" }
        );
        let launch = agents::resolve_on_node(agent, &paths).await.unwrap();
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

#[tokio::test]
async fn agent_launch_adds_configured_environment_with_global_path() {
    let agent = AgentRegistration {
        is_default: false,
        id: "custom".to_owned(),
        agent_type: Some(AgentType::Codex),
        display_name: "Custom Agent".to_owned(),
        source: "configured".into(),
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
    let launch = agents::resolve_on_node(agent, &paths).await.unwrap();
    assert_eq!(launch.env["PATH"], "/discovered/bin");
    assert_eq!(launch.env["AOW_AGENT_TEST"], "value");
    assert_eq!(launch.env["EMPTY"], "");
    assert_eq!(launch.env.len(), 4);
    assert!(launch.env.contains_key("AOW_STATE_DIR"));
}

fn test_agent_session(
    agent: &str,
    session_id: &str,
    cwd: &Path,
    _transcript_path: &Path,
    _trusted_root: &Path,
) -> aow_runtime_client::NativeSession {
    aow_runtime_client::NativeSession {
        id: session_id.into(),
        agent: agent.into(),
        session_id: session_id.into(),
        title: "Test session".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        created_at: String::new(),
        updated_at: String::new(),
        reference: aow_runtime_client::NativeSessionReference {
            node_id: "node".into(),
            handle: session_id.into(),
            agent: agent.into(),
            session_id: session_id.into(),
            cwd: cwd.into(),
            title: "Test session".into(),
        },
    }
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
    let manager = TestServices::in_memory();
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
        .agents
        .cache_native_sessions(&parent, None, &sessions, true)
        .unwrap();
    assert!(
        manager
            .agents
            .session_locator(&parent, "traecli", "session-1")
            .is_ok()
    );
    assert!(
        manager
            .agents
            .session_locator(&nested, "traecli", "session-1")
            .is_err()
    );
    assert!(
        manager
            .agents
            .session_locator(&parent, "codex", "session-1")
            .is_err()
    );

    manager
        .agents
        .cache_native_sessions(&parent, Some("traecli"), &[], true)
        .unwrap();
    assert!(
        manager
            .agents
            .session_locator(&parent, "traecli", "session-1")
            .is_err()
    );
    assert!(
        manager
            .agents
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

    let manager = TestServices::in_memory();
    let project = manager
        .projects
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
            .projects
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
    let manager = TestServices::persistent(&state_directory).unwrap();
    let project = manager
        .projects
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: Some(root.join("notes").to_string_lossy().into_owned()),
        })
        .await
        .unwrap();
    let worktree_path = root.join("repo-feature");
    let failed_pull = manager
        .projects
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
        manager
            .projects
            .project(&project.id)
            .await
            .unwrap()
            .worktrees
            .len(),
        1
    );

    let result = manager
        .projects
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
            .projects
            .project_name_for_workspace(repository.to_str().unwrap())
            .await,
        Some(project.name.clone())
    );
    assert_eq!(
        manager
            .projects
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
        .projects
        .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Cat))
        .await
        .unwrap();

    let colored = manager
        .projects
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
    let manager = TestServices::persistent(&state_directory).unwrap();
    let reloaded = manager.projects.project(&project.id).await.unwrap();
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
            .projects
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
                    .projects
                    .test_registry()
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
            .projects
            .set_worktree_icon(
                &project.id,
                SetWorktreeIconRequest {
                    path: root.join("not-a-worktree").to_string_lossy().into_owned(),
                    icon: WorktreeIcon::Cat,
                }
            )
            .await,
        Err(ServiceError::Invalid(_))
    ));

    // A failed disk write must restore the in-memory shape as well.
    let projects_path = manager.projects.test_registry_path().unwrap();
    let saved_projects = std::fs::read(projects_path).unwrap();
    std::fs::remove_file(projects_path).unwrap();
    std::fs::create_dir(projects_path).unwrap();
    assert!(
        manager
            .projects
            .set_worktree_icon(&project.id, icon_request(WorktreeIcon::Dog))
            .await
            .is_err()
    );
    let unchanged = manager.projects.project(&project.id).await.unwrap();
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
        .projects
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
    assert!(matches!(duplicate, Err(ServiceError::Git(_))));
    assert!(!duplicate_path.exists());

    let existing_path = root.join("already-exists");
    std::fs::create_dir(&existing_path).unwrap();
    let existing = manager
        .projects
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
    assert!(matches!(existing, Err(ServiceError::Invalid(_))));

    std::fs::write(worktree_path.join("uncommitted.txt"), b"not committed\n").unwrap();
    let inspection = manager
        .projects
        .inspect_worktree_removal(&project.id, worktree_path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(inspection.changes, vec!["?? uncommitted.txt"]);
    assert_eq!(inspection.change_count, 1);
    assert!(!inspection.truncated);

    let without_force = manager
        .projects
        .remove_worktree(&project.id, worktree_path.to_str().unwrap(), false)
        .await;
    assert!(matches!(without_force, Err(ServiceError::DirtyWorktree(1))));
    assert!(worktree_path.exists());

    let reregistered = manager
        .projects
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
        .projects
        .remove_worktree(&project.id, worktree_path.to_str().unwrap(), true)
        .await
        .unwrap();
    assert!(!worktree_path.exists());
    let document: RegistryDocument<StoredProject> = serde_json::from_slice(
        &std::fs::read(manager.projects.test_registry_path().unwrap()).unwrap(),
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
        .projects
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
        .projects
        .inspect_worktree_removal(&project.id, clean_path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(clean_inspection.change_count, 0);
    manager
        .projects
        .remove_worktree(&project.id, clean_path.to_str().unwrap(), false)
        .await
        .unwrap();
    assert!(!clean_path.exists());

    let main_removal = manager
        .projects
        .inspect_worktree_removal(&project.id, repository.to_str().unwrap())
        .await;
    assert!(matches!(main_removal, Err(ServiceError::Invalid(_))));
}
