use super::*;

impl AowManager {
    pub(in crate::aow) async fn inspect_worktree_removal(
        &self,
        id: &str,
        requested_path: &str,
    ) -> Result<WorktreeRemovalInspection, AowError> {
        let (_, _, worktree, _) = self.worktree_removal_target(id, requested_path).await?;
        let status = git_output(
            Path::new(&worktree.path),
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .await?;
        const MAX_REPORTED_CHANGES: usize = 100;
        let mut all_changes = status.lines().map(str::to_owned).collect::<Vec<_>>();
        let change_count = all_changes.len();
        let truncated = change_count > MAX_REPORTED_CHANGES;
        all_changes.truncate(MAX_REPORTED_CHANGES);
        Ok(WorktreeRemovalInspection {
            worktree,
            changes: all_changes,
            change_count,
            truncated,
        })
    }

    #[cfg(test)]
    pub(in crate::aow) async fn remove_worktree(
        &self,
        id: &str,
        requested_path: &str,
        force: bool,
    ) -> Result<Project, AowError> {
        self.remove_worktree_files(id, requested_path, force)
            .await?;
        self.project(id).await
    }

    pub(in crate::aow) async fn remove_worktree_files(
        &self,
        id: &str,
        requested_path: &str,
        force: bool,
    ) -> Result<(), AowError> {
        let (stored, _, worktree, git_cwd) =
            self.worktree_removal_target(id, requested_path).await?;
        let status = git_output(
            Path::new(&worktree.path),
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .await?;
        let dirty_count = status.lines().count();
        if dirty_count > 0 && !force {
            return Err(AowError::DirtyWorktree(dirty_count));
        }

        let (previous_registered_path, previous_color, previous_icon) = {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous_registered_path =
                (Path::new(&stored.registered_path) == Path::new(&worktree.path)).then(|| {
                    std::mem::replace(
                        &mut state.projects[index].registered_path,
                        git_cwd.to_string_lossy().into_owned(),
                    )
                });
            let previous_color = state.projects[index].worktree_colors.remove(&worktree.path);
            let previous_icon = state.projects[index].worktree_icons.remove(&worktree.path);
            if (previous_registered_path.is_some()
                || previous_color.is_some()
                || previous_icon.is_some())
                && let Err(error) = self.persist_projects(&state.projects)
            {
                if let Some(previous) = previous_registered_path {
                    state.projects[index].registered_path = previous;
                }
                if let Some(color) = previous_color {
                    state.projects[index]
                        .worktree_colors
                        .insert(worktree.path.clone(), color);
                }
                if let Some(icon) = previous_icon {
                    state.projects[index]
                        .worktree_icons
                        .insert(worktree.path.clone(), icon);
                }
                return Err(error);
            }
            (previous_registered_path, previous_color, previous_icon)
        };

        let path = worktree.path.as_str();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.extend(["--", path]);
        if let Err(error) = git_output(&git_cwd, &args).await {
            if previous_registered_path.is_some()
                || previous_color.is_some()
                || previous_icon.is_some()
            {
                let mut state = self.lock()?;
                if let Some(project) = state.projects.iter_mut().find(|project| project.id == id) {
                    if let Some(previous) = previous_registered_path {
                        project.registered_path = previous;
                    }
                    if let Some(color) = previous_color {
                        project.worktree_colors.insert(worktree.path.clone(), color);
                    }
                    if let Some(icon) = previous_icon {
                        project.worktree_icons.insert(worktree.path.clone(), icon);
                    }
                    self.persist_projects(&state.projects)?;
                }
            }
            return Err(error);
        }
        Ok(())
    }

    async fn worktree_removal_target(
        &self,
        id: &str,
        requested_path: &str,
    ) -> Result<(StoredProject, Project, Worktree, PathBuf), AowError> {
        let requested_path = requested_path.trim();
        if requested_path.is_empty()
            || requested_path.len() > 4096
            || requested_path.contains('\0')
            || !Path::new(requested_path).is_absolute()
        {
            return Err(AowError::Invalid(
                "worktree path must be an absolute path".to_owned(),
            ));
        }
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let project = super::projects::project_from_stored_strict(stored.clone()).await?;
        let worktree = project
            .worktrees
            .iter()
            .find(|worktree| worktree.path == requested_path)
            .cloned()
            .ok_or_else(|| {
                AowError::Invalid(format!(
                    "path is not a worktree in project {}: {requested_path}",
                    project.name
                ))
            })?;
        if worktree.is_main {
            return Err(AowError::Invalid(
                "the main worktree cannot be removed".to_owned(),
            ));
        }
        if worktree.locked {
            return Err(AowError::Invalid(
                "locked worktrees must be unlocked before removal".to_owned(),
            ));
        }
        let git_cwd = project
            .worktrees
            .iter()
            .find(|candidate| candidate.is_main)
            .map(|candidate| PathBuf::from(&candidate.path))
            .ok_or_else(|| AowError::Git("project has no main worktree".to_owned()))?;
        Ok((stored, project, worktree, git_cwd))
    }
}
