use super::*;

impl AowManager {
    pub(in crate::aow) async fn update_notes_settings(
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
                        None => notes::default_notes_identity(Path::new(&project.registered_path))
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
