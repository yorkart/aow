use super::*;
use crate::settings::{UpdateSettingsRequest, persistence::load_registry};
use crate::test_support::{TestServices, git_output};

#[tokio::test]
async fn relative_application_home_initializes_with_absolute_notes_paths() {
    let current = std::env::current_dir().unwrap();
    let temporary = tempfile::Builder::new()
        .prefix(".aow-relative-home-")
        .tempdir_in(&current)
        .unwrap();
    let relative = temporary
        .path()
        .strip_prefix(&current)
        .unwrap()
        .join("home");
    let manager = TestServices::persistent(&relative).unwrap();
    manager.projects.initialize_global().await.unwrap();
    let expected = temporary
        .path()
        .join("home/data/notes")
        .canonicalize()
        .unwrap();
    assert_eq!(
        Path::new(&manager.settings.settings().unwrap().notes_base),
        expected
    );
    let project = manager.projects.project(FLOATING_PROJECT_ID).await.unwrap();
    assert!(Path::new(&project.notes_path).starts_with(&expected));
    assert!(Path::new(&project.notes_path).is_dir());
}

#[tokio::test]
async fn initializes_real_global_repository_once_and_reuses_regular_project_registration() {
    let temp = tempfile::tempdir().unwrap();
    let manager = TestServices::persistent_with_notes_base(
        &temp.path().join("state"),
        temp.path().join("notes"),
    )
    .unwrap();
    manager.projects.initialize_global().await.unwrap();
    let project = manager.projects.projects().await.unwrap().remove(0);
    assert_eq!(project.id, FLOATING_PROJECT_ID);
    assert!(project.builtin);
    assert_eq!(project.worktrees.len(), 1);
    assert_eq!(project.worktrees[0].project_id, FLOATING_PROJECT_ID);
    assert_eq!(project.worktrees[0].branch, "main");
    assert_eq!(
        Path::new(&project.registered_path),
        std::fs::canonicalize(temp.path().join("state/data/repos/__aow_floating")).unwrap()
    );
    assert!(Path::new(&project.registered_path).join(".git").is_dir());
    assert_eq!(
        Path::new(&project.notes_path).canonicalize().unwrap(),
        temp.path()
            .join("notes/localhost")
            .join(TestServices::in_memory().node.node_account().await.unwrap())
            .join(FLOATING_PROJECT_ID)
            .canonicalize()
            .unwrap()
    );
    std::fs::write(Path::new(&project.registered_path).join("keep.txt"), "keep").unwrap();
    git_output(
        Path::new(&project.registered_path),
        &["symbolic-ref", "HEAD", "refs/heads/existing"],
    )
    .await
    .unwrap();
    manager.projects.initialize_global().await.unwrap();
    assert_eq!(manager.projects.projects().await.unwrap().len(), 1);
    assert_eq!(
        manager
            .projects
            .project(FLOATING_PROJECT_ID)
            .await
            .unwrap()
            .worktrees[0]
            .branch,
        "existing"
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&project.registered_path).join("keep.txt")).unwrap(),
        "keep"
    );
    assert!(
        manager
            .projects
            .remove_project(FLOATING_PROJECT_ID)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn restored_configuration_replaces_stale_builtin_identity_and_uses_the_local_repository() {
    let temp = tempfile::tempdir().unwrap();
    let original = TestServices::persistent_with_notes_base(
        &temp.path().join("original-state"),
        temp.path().join("notes"),
    )
    .unwrap();
    original.projects.initialize_global().await.unwrap();
    let old_project = original
        .projects
        .project(FLOATING_PROJECT_ID)
        .await
        .unwrap();
    {
        let mut registry = original.projects.test_registry().unwrap();
        registry
            .projects
            .iter_mut()
            .find(|project| project.builtin)
            .unwrap()
            .id = "stale-floating-project".into();
        original
            .projects
            .persist_projects(&registry.projects)
            .unwrap();
    }
    let config = original.projects.test_config().unwrap();
    let state = temp.path().join("new-state");
    std::fs::create_dir(&state).unwrap();
    std::fs::copy(
        config.directory().join("aow-projects.json"),
        state.join("aow-projects.json"),
    )
    .unwrap();
    let restored =
        TestServices::persistent_with_notes_base(&state, temp.path().join("notes")).unwrap();
    restored.projects.initialize_global().await.unwrap();
    restored.projects.initialize_global().await.unwrap();
    let projects = restored.projects.projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, FLOATING_PROJECT_ID);
    assert_eq!(
        Path::new(&projects[0].registered_path),
        std::fs::canonicalize(state.join("data/repos/__aow_floating")).unwrap()
    );
    assert!(
        Path::new(&old_project.registered_path)
            .join(".git")
            .is_dir()
    );
}

#[tokio::test]
async fn notes_root_preserves_builtin_and_custom_bindings_and_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let base = root.join("notes");
    let manager =
        TestServices::persistent_with_notes_base(&root.join("state"), base.clone()).unwrap();
    manager.projects.initialize_global().await.unwrap();
    let repository = root.join("ordinary");
    std::fs::create_dir(&repository).unwrap();
    git_output(&repository, &["init"]).await.unwrap();
    let normal = manager
        .projects
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let before = manager.projects.projects().await.unwrap();
    for project in &before {
        std::fs::write(Path::new(&project.notes_path).join("note.md"), &project.id).unwrap();
    }
    let custom = root.join("custom");
    manager
        .projects
        .bind_notes(&normal.id, custom.to_str().unwrap())
        .await
        .unwrap();
    let next = root.join("new-notes");
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(next.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    let global = manager.projects.project(FLOATING_PROJECT_ID).await.unwrap();
    assert_eq!(global.notes_path, before[0].notes_path);
    assert!(!Path::new(&global.notes_path).is_symlink());
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 0);
    assert_eq!(global.registered_path, before[0].registered_path);
    assert_eq!(
        Path::new(
            &manager
                .projects
                .project(&normal.id)
                .await
                .unwrap()
                .notes_path
        )
        .canonicalize()
        .unwrap(),
        custom.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&before[0].notes_path).join("note.md")).unwrap(),
        FLOATING_PROJECT_ID
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&normal.notes_path).join("note.md")).unwrap(),
        normal.id
    );
    assert_eq!(std::fs::read_dir(&custom).unwrap().count(), 0);
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(base.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        manager
            .projects
            .project(FLOATING_PROJECT_ID)
            .await
            .unwrap()
            .notes_path,
        before[0].notes_path
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&global.notes_path).join("note.md")).unwrap(),
        FLOATING_PROJECT_ID
    );
}

#[tokio::test]
async fn notes_root_accepts_existing_notes_without_changing_project_bindings_or_files() {
    let temp = tempfile::tempdir().unwrap();
    let manager = TestServices::persistent_with_notes_base(
        &temp.path().join("state"),
        temp.path().join("notes"),
    )
    .unwrap();
    manager.projects.initialize_global().await.unwrap();
    let before = manager.projects.project(FLOATING_PROJECT_ID).await.unwrap();
    let next = temp.path().join("new");
    let target = next
        .join("localhost")
        .join(TestServices::in_memory().node.node_account().await.unwrap())
        .join(FLOATING_PROJECT_ID);
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.md"), "keep").unwrap();
    assert_eq!(std::fs::read_dir(&before.notes_path).unwrap().count(), 0);
    let settings = manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(next.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        Path::new(&settings.notes_base),
        next.canonicalize().unwrap()
    );
    assert_eq!(
        manager
            .projects
            .project(FLOATING_PROJECT_ID)
            .await
            .unwrap()
            .notes_path,
        before.notes_path
    );
    assert_eq!(
        std::fs::read_to_string(target.join("keep.md")).unwrap(),
        "keep"
    );
    assert!(Path::new(&before.notes_path).is_dir());
    assert!(!Path::new(&before.notes_path).is_symlink());
    assert_eq!(std::fs::read_dir(&before.notes_path).unwrap().count(), 0);
}
#[tokio::test]
async fn notes_root_preserves_default_and_legacy_bindings_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let base = root.join("notes");
    let state_dir = root.join("state");
    let manager = TestServices::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    manager.projects.initialize_global().await.unwrap();
    for name in ["ordinary", "legacy"] {
        let repository = root.join(name);
        std::fs::create_dir(&repository).unwrap();
        git_output(&repository, &["init"]).await.unwrap();
        let project = manager
            .projects
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        std::fs::write(Path::new(&project.notes_path).join("note.md"), name).unwrap();
        if name == "legacy" {
            let mut state = manager.projects.test_registry().unwrap();
            state
                .projects
                .iter_mut()
                .find(|item| item.id == project.id)
                .unwrap()
                .notes_identity = None;
            manager.projects.persist_projects(&state.projects).unwrap();
        }
    }
    let before = manager.projects.test_registry().unwrap().projects.clone();
    let registry_path = manager.projects.test_registry_path().unwrap();
    let registry_before = std::fs::read(registry_path).unwrap();
    let next = root.join("new-notes");
    manager
        .settings
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(next.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(std::fs::read(registry_path).unwrap(), registry_before);
    let reloaded = TestServices::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    reloaded.projects.initialize_global().await.unwrap();
    assert_eq!(
        Path::new(&reloaded.settings.settings().unwrap().notes_base),
        next
    );
    for project in reloaded.projects.projects().await.unwrap() {
        let project_notes = Path::new(&project.notes_path).canonicalize().unwrap();
        let previous = before.iter().find(|item| item.id == project.id).unwrap();
        assert_eq!(project.notes_path, previous.notes_path);
        assert!(project_notes.starts_with(&base));
        assert!(!Path::new(&project.notes_path).is_symlink());
        if !project.builtin {
            assert_eq!(
                std::fs::read_to_string(Path::new(&project.notes_path).join("note.md")).unwrap(),
                project.name
            );
        }
    }
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 0);
}

#[tokio::test]
async fn failed_settings_persistence_preserves_settings_notes_and_project_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let manager =
        TestServices::persistent_with_notes_base(&state_dir, temp.path().join("notes")).unwrap();
    manager.projects.initialize_global().await.unwrap();
    let previous_settings = manager.settings.settings().unwrap();
    let before = manager.projects.project(FLOATING_PROJECT_ID).await.unwrap();
    let source = Path::new(&before.notes_path);
    std::fs::write(source.join("note.md"), "unsaved work").unwrap();
    std::fs::create_dir(manager.settings.test_settings_path().unwrap()).unwrap();
    let next = temp.path().join("new-notes");
    assert!(
        manager
            .settings
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(next.to_string_lossy().into_owned()),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert_eq!(
        manager.settings.settings().unwrap().notes_base,
        previous_settings.notes_base
    );
    assert_eq!(
        manager.settings.settings().unwrap().execution_path,
        previous_settings.execution_path
    );
    assert!(!source.is_symlink());
    assert_eq!(
        std::fs::read_to_string(source.join("note.md")).unwrap(),
        "unsaved work"
    );
    assert_eq!(
        manager
            .projects
            .project(FLOATING_PROJECT_ID)
            .await
            .unwrap()
            .notes_path,
        before.notes_path
    );
    let persisted: Vec<StoredProject> =
        load_registry(manager.projects.test_registry_path().unwrap()).unwrap();
    assert_eq!(persisted[0].notes_path, before.notes_path);
    assert!(
        !next
            .join("localhost")
            .join(TestServices::in_memory().node.node_account().await.unwrap())
            .join(FLOATING_PROJECT_ID)
            .exists()
    );
}

#[tokio::test]
async fn binding_notes_uses_existing_contents_or_creates_an_empty_directory_without_moving_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let state_dir = root.join("state");
    let base = root.join("notes");
    let manager = TestServices::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    manager.projects.initialize_global().await.unwrap();
    let before = manager.projects.project(FLOATING_PROJECT_ID).await.unwrap();
    let source = Path::new(&before.notes_path);
    std::fs::write(source.join("note.md"), "original note").unwrap();
    std::fs::write(source.join("old-only.md"), "keep in old directory").unwrap();
    let existing = root.join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("note.md"), "existing note").unwrap();

    let bound = manager
        .projects
        .bind_notes(&before.id, existing.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(Path::new(&bound.notes_path), existing);
    assert_eq!(
        std::fs::read_to_string(existing.join("note.md")).unwrap(),
        "existing note"
    );
    assert!(!existing.join("old-only.md").exists());
    assert_eq!(
        std::fs::read_to_string(source.join("note.md")).unwrap(),
        "original note"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("old-only.md")).unwrap(),
        "keep in old directory"
    );
    assert!(!source.is_symlink());

    let missing = root.join("new/nested/notes");
    let bound = manager
        .projects
        .bind_notes(&before.id, missing.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(Path::new(&bound.notes_path), missing);
    assert_eq!(std::fs::read_dir(&missing).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_to_string(existing.join("note.md")).unwrap(),
        "existing note"
    );
    let reloaded = TestServices::persistent_with_notes_base(&state_dir, base).unwrap();
    assert_eq!(
        reloaded
            .projects
            .project(&before.id)
            .await
            .unwrap()
            .notes_path,
        bound.notes_path
    );
    assert!(
        manager
            .projects
            .bind_notes(&before.id, existing.join("note.md").to_str().unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        manager
            .projects
            .project(&before.id)
            .await
            .unwrap()
            .notes_path,
        bound.notes_path
    );
}
