use super::*;

fn validate_git_value(label: &str, value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(format!(
            "{label} must contain 1-1024 printable characters"
        )));
    }
    Ok(value.to_owned())
}
impl AowManager {
    pub(in crate::aow) async fn create_worktree(
        &self,
        id: &str,
        request: CreateWorktreeRequest,
    ) -> Result<CreateWorktreeResponse, AowError> {
        let project_lock = self.inner.removals.project_lock(id)?;
        let _project = project_lock.lock().await;
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let branch = validate_git_value("branch", &request.branch)?;
        let base_ref = validate_git_value("base ref", &request.base_ref)?;
        let path = paths::validate_new_worktree_path(&request.path).await?;
        let registered_path = PathBuf::from(&stored.registered_path);

        git_output(&registered_path, &["check-ref-format", "--branch", &branch]).await?;
        if request.pull_first {
            let output = git_output(&registered_path, &["worktree", "list", "--porcelain"]).await?;
            let main_worktree = parse_worktrees(id, &output)
                .into_iter()
                .find(|worktree| worktree.is_main)
                .ok_or_else(|| AowError::Git("project has no main worktree".to_owned()))?;
            git_output(Path::new(&main_worktree.path), &["pull"])
                .await
                .map_err(|error| {
                    AowError::Git(format!(
                        "git pull failed in main worktree {}: {error}\n可先在终端检查 git pull，或取消勾选「创建前更新主仓库」后重试。",
                        main_worktree.path
                    ))
                })?;
        }
        let path_value = path.to_string_lossy().into_owned();
        git_output(
            &registered_path,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                "--",
                &path_value,
                &base_ref,
            ],
        )
        .await?;

        let created_path = paths::canonical_directory(&path).await?;
        let project = super::projects::project_from_stored_strict(stored).await?;
        let worktree = project
            .worktrees
            .iter()
            .find(|worktree| Path::new(&worktree.path) == created_path)
            .cloned()
            .ok_or_else(|| {
                AowError::Git(format!(
                    "created worktree was not returned by git worktree list: {}",
                    created_path.display()
                ))
            })?;
        Ok(CreateWorktreeResponse { project, worktree })
    }

    pub(in crate::aow) async fn set_worktree_color(
        &self,
        id: &str,
        request: SetWorktreeColorRequest,
    ) -> Result<Project, AowError> {
        let mut project = self.project(id).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| worktree.path == request.path)
        {
            return Err(AowError::Invalid(format!(
                "path is not a worktree in project {}: {}",
                project.name, request.path
            )));
        }

        {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index]
                .worktree_colors
                .get(&request.path)
                .copied();
            if request.color == WorktreeColor::Default {
                state.projects[index].worktree_colors.remove(&request.path);
            } else {
                state.projects[index]
                    .worktree_colors
                    .insert(request.path.clone(), request.color);
            }
            if let Err(error) = self.persist_projects(&state.projects) {
                match previous {
                    Some(color) => {
                        state.projects[index]
                            .worktree_colors
                            .insert(request.path, color);
                    }
                    None => {
                        state.projects[index].worktree_colors.remove(&request.path);
                    }
                }
                return Err(error);
            }
        }
        if let Some(worktree) = project
            .worktrees
            .iter_mut()
            .find(|worktree| worktree.path == request.path)
        {
            worktree.color = request.color;
        }
        Ok(project)
    }

    pub(in crate::aow) async fn set_worktree_icon(
        &self,
        id: &str,
        request: SetWorktreeIconRequest,
    ) -> Result<Project, AowError> {
        let mut project = self.project(id).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| worktree.path == request.path)
        {
            return Err(AowError::Invalid(format!(
                "path is not a worktree in project {}: {}",
                project.name, request.path
            )));
        }

        {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index]
                .worktree_icons
                .get(&request.path)
                .copied();
            if request.icon == WorktreeIcon::Default {
                state.projects[index].worktree_icons.remove(&request.path);
            } else {
                state.projects[index]
                    .worktree_icons
                    .insert(request.path.clone(), request.icon);
            }
            if let Err(error) = self.persist_projects(&state.projects) {
                match previous {
                    Some(icon) => {
                        state.projects[index]
                            .worktree_icons
                            .insert(request.path, icon);
                    }
                    None => {
                        state.projects[index].worktree_icons.remove(&request.path);
                    }
                }
                return Err(error);
            }
        }
        if let Some(worktree) = project
            .worktrees
            .iter_mut()
            .find(|worktree| worktree.path == request.path)
        {
            worktree.icon = request.icon;
        }
        Ok(project)
    }
}
