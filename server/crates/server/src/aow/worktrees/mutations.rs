use super::*;

impl AowManager {
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
