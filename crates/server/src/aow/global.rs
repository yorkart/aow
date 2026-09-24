use super::*;

const FLOATING_PROJECT_ID: &str = "__aow_floating";

impl AowManager {
    pub(crate) async fn initialize_global(&self) -> Result<(), AowError> {
        let Some(state_directory) = &self.inner.state_dir else {
            return Ok(());
        };
        let _operation = self.inner.project_operation.lock().await;
        let directory = state_directory.join("repos").join(FLOATING_PROJECT_ID);
        tokio::fs::create_dir_all(&directory).await?;
        if !directory.join(".git").exists() {
            // Keep initialization compatible with Git versions before 2.28.
            git_output(&directory, &["init"]).await?;
            git_output(&directory, &["symbolic-ref", "HEAD", "refs/heads/main"]).await?;
        }
        let project = self
            .register_project_inner(RegisterProjectRequest {
                path: directory.to_string_lossy().into_owned(),
                name: None,
                notes_path: None,
            })
            .await?;
        let mut state = self.lock()?;
        let index = state
            .projects
            .iter()
            .position(|p| p.id == project.id)
            .unwrap();
        let mut project = state.projects.remove(index);
        project.id = FLOATING_PROJECT_ID.into();
        project.name = "浮动工作区".into();
        project.builtin = true;
        // There is one floating builtin project. Restored records can have a
        // different identity or another machine's repository path.
        state
            .projects
            .retain(|project| !project.builtin && project.id != FLOATING_PROJECT_ID);
        state.projects.insert(0, project);
        self.persist_projects(&state.projects)
    }

    pub(super) async fn update_notes_settings(
        &self,
        base: Option<PathBuf>,
        execution_path: Option<Vec<PathBuf>>,
        editor: Option<EditorSettings>,
        node_addresses: Option<Vec<String>>,
        operation: tokio::sync::OwnedMutexGuard<()>,
    ) -> Result<AowSettings, AowError> {
        let previous = self.settings()?;
        let mut projects = self.lock()?.projects.clone();
        let mut moves = Vec::new();
        if let Some(base) = &base {
            if base != Path::new(&previous.notes_base) {
                for project in &mut projects {
                    if project.notes_custom {
                        continue;
                    }
                    let identity = match &project.notes_identity {
                        Some(identity) => Some(identity.clone()),
                        None => default_notes_identity(Path::new(&project.registered_path))
                            .await
                            .ok()
                            .filter(|identity| {
                                Path::new(&previous.notes_base).join(identity)
                                    == Path::new(&project.notes_path)
                            }),
                    };
                    if let Some(identity) = identity {
                        let target = base.join(&identity);
                        let source = PathBuf::from(&project.notes_path);
                        if target != source {
                            moves.push((source, target.clone()));
                        }
                        project.notes_identity = Some(identity);
                        project.notes_path = target.to_string_lossy().into_owned();
                    }
                }
            }
        }
        moves.sort();
        moves.dedup();
        let filesystem = self.inner.filesystem_operation.clone().write_owned().await;
        let manager = self.clone();
        tokio::task::spawn_blocking(move || {
            // Keep both guards until disk work completes, even if the HTTP request is cancelled.
            let (_operation, _filesystem) = (operation, filesystem);
            // Preflight every destination before changing any project binding.
            for (source, target) in &moves {
                if target.starts_with(source) || source.starts_with(target) {
                    return Err(AowError::Invalid("Notes 目录不能迁入自身或父目录".into()));
                }
                if moves.iter().any(|(other, _)| other != source && (target.starts_with(other) || other.starts_with(target))) {
                    return Err(AowError::Invalid("Notes 目标目录与其他项目的目录重叠".into()));
                }
                if target.symlink_metadata().is_ok() && !(target.is_symlink() && std::fs::canonicalize(target).ok() == std::fs::canonicalize(source).ok()) {
                    return Err(AowError::Invalid(format!("Notes 目标目录已存在，请先处理冲突：{}", target.display())));
                }
            }
            let mut completed = Vec::new();
            let mut aliases = Vec::new();
            let result = (|| {
                for (source, target) in &moves {
                    std::fs::create_dir_all(target.parent().unwrap())?;
                    if target.is_symlink() {
                        aliases.push((target.clone(), std::fs::read_link(target)?));
                        std::fs::remove_file(target)?;
                    }
                    let moved = NotesMove::apply(source, target)?;
                    completed.push(moved);
                    std::fs::create_dir_all(source.parent().unwrap())?;
                    // Existing browser tabs and in-flight saves retain a valid path.
                    std::os::unix::fs::symlink(target, source)?;
                }
                let mut settings = manager.lock_settings()?;
                let mut state = manager.lock()?;
                let mut next = settings.clone();
                if let Some(base) = base { next.notes_base = base.to_string_lossy().into_owned(); }
                if let Some(path) = execution_path { next.execution_path = Some(path); }
                if let Some(editor) = editor { next.editor = editor; }
                if let Some(addresses) = node_addresses { next.node_addresses = addresses; }
                // Worktree color changes can occur while files are moving; preserve them.
                let mut next_projects = state.projects.clone();
                for project in &mut next_projects {
                    if let Some(binding) = projects.iter().find(|item| item.id == project.id) {
                        project.notes_path.clone_from(&binding.notes_path);
                        project.notes_identity.clone_from(&binding.notes_identity);
                    }
                }
                manager.persist_projects(&next_projects)?;
                if let Err(error) = manager.persist_settings(&next) {
                    manager.persist_projects(&state.projects)?;
                    return Err(error);
                }
                state.projects = next_projects;
                *settings = next.clone();
                Ok(next)
            })();
            if result.is_err() {
                for moved in completed.iter().rev() {
                    if let Err(error) = moved.rollback() {
                        tracing::error!(%error, path = %moved.target.display(), "Notes rollback retained data at destination");
                    }
                }
                for (alias, target) in aliases { let _ = std::os::unix::fs::symlink(target, alias); }
            } else {
                for moved in &completed {
                    if let Some(backup) = &moved.backup {
                        if let Err(error) = std::fs::remove_dir_all(backup) {
                            tracing::warn!(%error, path = %backup.display(), "Notes migration retained backup");
                        }
                    }
                }
            }
            result
        }).await.map_err(|error| AowError::Invalid(error.to_string()))?
    }
}

struct NotesMove {
    source: PathBuf,
    target: PathBuf,
    backup: Option<PathBuf>,
    existed: bool,
}

impl NotesMove {
    fn apply(source: &Path, target: &Path) -> Result<Self, std::io::Error> {
        let mut moved = Self {
            source: source.into(),
            target: target.into(),
            backup: None,
            existed: source.exists(),
        };
        if !moved.existed {
            std::fs::create_dir(target)?;
            return Ok(moved);
        }
        match std::fs::rename(source, target) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
                if let Err(error) = copy_directory(source, target) {
                    let _ = std::fs::remove_dir_all(target);
                    return Err(error);
                }
                // Retain the complete source until the bindings have been committed.
                let backup =
                    source.with_file_name(format!(".aow-notes-migration-{}", Uuid::new_v4()));
                if let Err(error) = std::fs::rename(source, &backup) {
                    let _ = std::fs::remove_dir_all(target);
                    return Err(error);
                }
                moved.backup = Some(backup);
            }
            Err(error) => return Err(error),
        }
        Ok(moved)
    }

    fn rollback(&self) -> Result<(), std::io::Error> {
        if self.source.is_symlink() {
            std::fs::remove_file(&self.source)?;
        }
        if let Some(backup) = &self.backup {
            std::fs::rename(backup, &self.source)?;
            std::fs::remove_dir_all(&self.target)
        } else if self.existed {
            std::fs::rename(&self.target, &self.source)
        } else {
            std::fs::remove_dir_all(&self.target)
        }
    }
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let dest = target.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, dest)?;
        } else if kind.is_dir() {
            copy_directory(&entry.path(), &dest)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &dest)?;
        } else {
            return Err(std::io::Error::other("Notes contains a non-regular file"));
        }
    }
    std::fs::set_permissions(target, std::fs::metadata(source)?.permissions())
}

#[cfg(test)]
mod tests {
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
            Path::new(&project.notes_path),
            temp.path()
                .join("notes/localhost")
                .join(process_account_name().unwrap())
                .join(FLOATING_PROJECT_ID)
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
    async fn restored_configuration_replaces_stale_builtin_identity_and_uses_the_local_repository()
    {
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
    async fn notes_root_moves_default_projects_and_preserves_custom_bindings_and_old_file_paths() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("notes");
        let manager =
            AowManager::persistent_with_notes_base(&temp.path().join("state"), base.clone())
                .unwrap();
        manager.initialize_global().await.unwrap();
        let repository = temp.path().join("ordinary");
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
        let custom = temp.path().join("custom");
        manager
            .bind_notes(&normal.id, custom.to_str().unwrap())
            .await
            .unwrap();
        let next = temp.path().join("new-notes");
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(next.to_string_lossy().into_owned()),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .unwrap();
        let global = manager.project(FLOATING_PROJECT_ID).await.unwrap();
        assert!(Path::new(&global.notes_path).starts_with(&next));
        assert_eq!(global.registered_path, before[0].registered_path);
        assert_eq!(
            manager.project(&normal.id).await.unwrap().notes_path,
            custom.to_string_lossy()
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&before[0].notes_path).join("note.md")).unwrap(),
            FLOATING_PROJECT_ID
        );
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
    async fn notes_conflict_preserves_files_and_settings() {
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
            .join(process_account_name().unwrap())
            .join(FLOATING_PROJECT_ID);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.md"), "keep").unwrap();
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
    }
    #[tokio::test]
    async fn notes_root_migrates_ordinary_and_legacy_default_bindings() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("notes");
        let state_dir = temp.path().join("state");
        let manager = AowManager::persistent_with_notes_base(&state_dir, base.clone()).unwrap();
        manager.initialize_global().await.unwrap();
        for name in ["ordinary", "legacy"] {
            let repository = temp.path().join(name);
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
        let next = temp.path().join("new-notes");
        manager
            .update_settings(UpdateSettingsRequest {
                notes_base: Some(next.to_string_lossy().into_owned()),
                execution_path: Some(vec![PathBuf::from("/usr/bin")]),
                ..Default::default()
            })
            .await
            .unwrap();
        let reloaded = AowManager::persistent_with_notes_base(&state_dir, base).unwrap();
        for project in reloaded.projects().await.unwrap() {
            assert!(Path::new(&project.notes_path).starts_with(&next));
            if !project.builtin {
                assert_eq!(
                    std::fs::read_to_string(Path::new(&project.notes_path).join("note.md"))
                        .unwrap(),
                    project.name
                );
            }
        }
    }

    #[tokio::test]
    async fn failed_settings_persistence_rolls_back_notes_and_project_bindings() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = temp.path().join("state");
        let manager =
            AowManager::persistent_with_notes_base(&state_dir, temp.path().join("notes")).unwrap();
        manager.initialize_global().await.unwrap();
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
                .join(process_account_name().unwrap())
                .join(FLOATING_PROJECT_ID)
                .exists()
        );
    }
}
