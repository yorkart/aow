use super::*;

#[tokio::test]
async fn initializes_real_global_repository_once_and_reuses_regular_project_registration() {
    let temp = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent_with_notes_base(
        &temp.path().join("state"),
        temp.path().join("notes"),
    )
    .unwrap();
    manager.initialize_global().await.unwrap();
    let project = manager.projects().await.unwrap().remove(0);
    assert_eq!(project.id, FLOATING_PROJECT_ID);
    assert!(project.builtin);
    assert_eq!(project.worktrees.len(), 1);
    assert_eq!(project.worktrees[0].project_id, FLOATING_PROJECT_ID);
    assert_eq!(project.worktrees[0].branch, "main");
    assert_eq!(
        Path::new(&project.registered_path),
        std::fs::canonicalize(temp.path().join("state/repos/__aow_floating")).unwrap()
    );
    assert!(Path::new(&project.registered_path).join(".git").is_dir());
    assert_eq!(
        Path::new(&project.notes_path).canonicalize().unwrap(),
        temp.path()
            .join("notes/localhost")
            .join(notes::process_account_name().unwrap())
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
    manager.initialize_global().await.unwrap();
    assert_eq!(manager.projects().await.unwrap().len(), 1);
    assert_eq!(
        manager
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
    assert!(manager.remove_project(FLOATING_PROJECT_ID).await.is_err());
}

#[tokio::test]
async fn restored_configuration_replaces_stale_builtin_identity_and_uses_the_local_repository() {
    let temp = tempfile::tempdir().unwrap();
    let original = AowManager::persistent_with_notes_base(
        &temp.path().join("original-state"),
        temp.path().join("notes"),
    )
    .unwrap();
    original.initialize_global().await.unwrap();
    let old_project = original.project(FLOATING_PROJECT_ID).await.unwrap();
    {
        let mut registry = original.lock().unwrap();
        registry
            .projects
            .iter_mut()
            .find(|project| project.builtin)
            .unwrap()
            .id = "stale-floating-project".into();
        original.persist_projects(&registry.projects).unwrap();
    }
    let config = original.inner.config.as_ref().unwrap();
    let clone = temp.path().join("cloned-config");
    git_output(
        temp.path(),
        &[
            "clone",
            config.directory().parent().unwrap().to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    )
    .await
    .unwrap();
    let state = temp.path().join("new-state");
    std::fs::create_dir(&state).unwrap();
    let selected = clone.join(config.directory().file_name().unwrap());
    aow_config::save_selection(
        &state,
        &aow_config::ConfigSelection {
            config_repo: clone.clone(),
            config_id: config.selection().config_id,
        },
    )
    .unwrap();
    let restored =
        AowManager::persistent_with_notes_base(&state, temp.path().join("notes")).unwrap();
    restored.initialize_global().await.unwrap();
    restored.initialize_global().await.unwrap();
    let projects = restored.projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, FLOATING_PROJECT_ID);
    assert_eq!(
        Path::new(&projects[0].registered_path),
        std::fs::canonicalize(state.join("repos/__aow_floating")).unwrap()
    );
    assert!(
        Path::new(&old_project.registered_path)
            .join(".git")
            .is_dir()
    );
    assert!(!selected.join("repos").exists());
    assert!(!state.join("config-repo").exists());
    assert!(
        git_output(&clone, &["status", "--porcelain"])
            .await
            .unwrap()
            .trim()
            .is_empty()
    );
}

#[tokio::test]
async fn notes_root_preserves_builtin_and_custom_bindings_and_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let base = root.join("notes");
    let manager =
        AowManager::persistent_with_notes_base(&root.join("state"), base.clone()).unwrap();
    manager.initialize_global().await.unwrap();
    let repository = root.join("ordinary");
    std::fs::create_dir(&repository).unwrap();
    git_output(&repository, &["init"]).await.unwrap();
    let normal = manager
        .register_project(RegisterProjectRequest {
            path: repository.to_string_lossy().into_owned(),
            name: None,
            notes_path: None,
        })
        .await
        .unwrap();
    let before = manager.projects().await.unwrap();
    for project in &before {
        std::fs::write(Path::new(&project.notes_path).join("note.md"), &project.id).unwrap();
    }
    let custom = root.join("custom");
    manager
        .bind_notes(&normal.id, custom.to_str().unwrap())
        .await
        .unwrap();
    let next = root.join("new-notes");
    manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(next.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    let global = manager.project(FLOATING_PROJECT_ID).await.unwrap();
    assert_eq!(global.notes_path, before[0].notes_path);
    assert!(!Path::new(&global.notes_path).is_symlink());
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 0);
    assert_eq!(global.registered_path, before[0].registered_path);
    assert_eq!(
        Path::new(&manager.project(&normal.id).await.unwrap().notes_path)
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
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(base.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        manager
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
    let manager = AowManager::persistent_with_notes_base(
        &temp.path().join("state"),
        temp.path().join("notes"),
    )
    .unwrap();
    manager.initialize_global().await.unwrap();
    let before = manager.project(FLOATING_PROJECT_ID).await.unwrap();
    let next = temp.path().join("new");
    let target = next
        .join("localhost")
        .join(notes::process_account_name().unwrap())
        .join(FLOATING_PROJECT_ID);
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.md"), "keep").unwrap();
    assert_eq!(std::fs::read_dir(&before.notes_path).unwrap().count(), 0);
    let settings = manager
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
    let manager = AowManager::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    manager.initialize_global().await.unwrap();
    for name in ["ordinary", "legacy"] {
        let repository = root.join(name);
        std::fs::create_dir(&repository).unwrap();
        git_output(&repository, &["init"]).await.unwrap();
        let project = manager
            .register_project(RegisterProjectRequest {
                path: repository.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await
            .unwrap();
        std::fs::write(Path::new(&project.notes_path).join("note.md"), name).unwrap();
        if name == "legacy" {
            let mut state = manager.lock().unwrap();
            state
                .projects
                .iter_mut()
                .find(|item| item.id == project.id)
                .unwrap()
                .notes_identity = None;
            manager.persist_projects(&state.projects).unwrap();
        }
    }
    let before = manager.lock().unwrap().projects.clone();
    let registry_path = manager.inner.projects_path.as_ref().unwrap();
    let registry_before = std::fs::read(registry_path).unwrap();
    let next = root.join("new-notes");
    manager
        .update_settings(UpdateSettingsRequest {
            notes_base: Some(next.to_string_lossy().into_owned()),
            execution_path: Some(vec![PathBuf::from("/usr/bin")]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(std::fs::read(registry_path).unwrap(), registry_before);
    let reloaded = AowManager::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    reloaded.initialize_global().await.unwrap();
    assert_eq!(Path::new(&reloaded.settings().unwrap().notes_base), next);
    for project in reloaded.projects().await.unwrap() {
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
        AowManager::persistent_with_notes_base(&state_dir, temp.path().join("notes")).unwrap();
    manager.initialize_global().await.unwrap();
    let previous_settings = manager.settings().unwrap();
    let before = manager.project(FLOATING_PROJECT_ID).await.unwrap();
    let source = Path::new(&before.notes_path);
    std::fs::write(source.join("note.md"), "unsaved work").unwrap();
    std::fs::create_dir(manager.inner.settings_path.as_ref().unwrap()).unwrap();
    let next = temp.path().join("new-notes");
    assert!(
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(next.to_string_lossy().into_owned()),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert_eq!(
        manager.settings().unwrap().notes_base,
        previous_settings.notes_base
    );
    assert_eq!(
        manager.settings().unwrap().execution_path,
        previous_settings.execution_path
    );
    assert!(!source.is_symlink());
    assert_eq!(
        std::fs::read_to_string(source.join("note.md")).unwrap(),
        "unsaved work"
    );
    assert_eq!(
        manager
            .project(FLOATING_PROJECT_ID)
            .await
            .unwrap()
            .notes_path,
        before.notes_path
    );
    let persisted: Vec<StoredProject> =
        load_registry(manager.inner.projects_path.as_ref().unwrap()).unwrap();
    assert_eq!(persisted[0].notes_path, before.notes_path);
    assert!(
        !next
            .join("localhost")
            .join(notes::process_account_name().unwrap())
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
    let manager = AowManager::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
    manager.initialize_global().await.unwrap();
    let before = manager.project(FLOATING_PROJECT_ID).await.unwrap();
    let source = Path::new(&before.notes_path);
    std::fs::write(source.join("note.md"), "original note").unwrap();
    std::fs::write(source.join("old-only.md"), "keep in old directory").unwrap();
    let existing = root.join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("note.md"), "existing note").unwrap();

    let bound = manager
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
        .bind_notes(&before.id, missing.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(Path::new(&bound.notes_path), missing);
    assert_eq!(std::fs::read_dir(&missing).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_to_string(existing.join("note.md")).unwrap(),
        "existing note"
    );
    let reloaded = AowManager::persistent_with_notes_base(&state_dir, base).unwrap();
    assert_eq!(
        reloaded.project(&before.id).await.unwrap().notes_path,
        bound.notes_path
    );
    assert!(
        manager
            .bind_notes(&before.id, existing.join("note.md").to_str().unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        manager.project(&before.id).await.unwrap().notes_path,
        bound.notes_path
    );
}
