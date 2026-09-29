use std::collections::BTreeMap;

use super::*;

impl AowManager {
    /// Registry metadata only; callers discover worktrees directly through Git.
    pub(crate) fn registered_projects(&self) -> Result<Vec<ProjectSummary>, AowError> {
        Ok(self
            .lock()?
            .projects
            .iter()
            .map(ProjectSummary::from)
            .collect())
    }

    pub(crate) fn registered_project(&self, id: &str) -> Result<ProjectSummary, AowError> {
        self.lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .map(ProjectSummary::from)
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))
    }

    pub(in crate::aow) async fn projects(&self) -> Result<Vec<Project>, AowError> {
        let stored = self.lock()?.projects.clone();
        let mut projects = Vec::with_capacity(stored.len());
        for item in stored {
            projects.push(project_from_stored(item).await);
        }
        Ok(projects)
    }

    /// Resolve notification labels from the registered project, including
    /// linked worktrees whose directory name differs from the project name.
    pub(crate) async fn project_name_for_workspace(&self, root: &str) -> Option<String> {
        let projects = self.lock().ok()?.projects.clone();
        if let Some(project) = projects
            .iter()
            .find(|project| project.registered_path == root)
        {
            return Some(project.name.clone());
        }
        if projects.is_empty() {
            return None;
        }
        let common = resolve_common_git_dir(Path::new(root)).await.ok()?;
        projects
            .into_iter()
            .find(|project| project.common_git_dir == common)
            .map(|project| project.name)
    }

    pub(in crate::aow) async fn register_project(
        &self,
        request: RegisterProjectRequest,
    ) -> Result<Project, AowError> {
        let _operation = self.inner.project_operation.lock().await;
        self.register_project_inner(request).await
    }

    pub(in crate::aow) async fn register_project_inner(
        &self,
        request: RegisterProjectRequest,
    ) -> Result<Project, AowError> {
        let input = paths::validate_absolute_directory(&request.path).await?;
        let root = git_output(&input, &["rev-parse", "--show-toplevel"]).await?;
        let root = paths::canonical_directory(Path::new(root.trim())).await?;
        let registered_path = root.to_string_lossy().into_owned();
        let common_git_dir = resolve_common_git_dir(&root).await?;
        let default_name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Project")
            .to_owned();
        let requested_name = request
            .name
            .as_deref()
            .map(validation::validate_display_name)
            .transpose()?;
        let existing_notes_path = self
            .lock()?
            .projects
            .iter()
            .find(|project| {
                project.registered_path == registered_path
                    || (!project.common_git_dir.is_empty()
                        && project.common_git_dir == common_git_dir)
            })
            .map(|project| project.notes_path.clone());
        let notes_identity = if request.notes_path.is_none() && existing_notes_path.is_none() {
            Some(notes::default_notes_identity(&root).await?)
        } else {
            None
        };
        let notes_path = if let Some(path) = request.notes_path.as_deref() {
            notes::prepare_notes_directory(Path::new(path)).await?
        } else if let Some(existing_notes_path) = existing_notes_path {
            existing_notes_path
        } else {
            let base = PathBuf::from(self.settings()?.notes_base);
            let identity = notes::default_notes_identity(&root).await?;
            notes::prepare_notes_directory(&base.join(identity)).await?
        };

        let item = {
            let mut state = self.lock()?;
            if let Some(existing) = state.projects.iter_mut().find(|project| {
                project.registered_path == registered_path
                    || (!project.common_git_dir.is_empty()
                        && project.common_git_dir == common_git_dir)
            }) {
                if let Some(name) = requested_name {
                    if !existing.builtin {
                        existing.name = name;
                    }
                }
                if request.notes_path.is_some() {
                    existing.notes_identity = None;
                    existing.notes_custom = true;
                }
                existing.registered_path = registered_path;
                existing.common_git_dir = common_git_dir;
                existing.notes_path = notes_path;
                let result = existing.clone();
                self.persist_projects(&state.projects)?;
                result
            } else {
                let project = StoredProject {
                    id: aow_id::new_id(),
                    name: requested_name.unwrap_or_else(|| default_name.clone()),
                    registered_path,
                    common_git_dir,
                    notes_path,
                    notes_identity,
                    notes_custom: request.notes_path.is_some(),
                    builtin: false,
                    avatar_url: None,
                    worktree_colors: BTreeMap::new(),
                    worktree_icons: BTreeMap::new(),
                };
                state.projects.push(project.clone());
                self.persist_projects(&state.projects)?;
                project
            }
        };
        project_from_stored_strict(item).await
    }

    pub(in crate::aow) async fn bind_notes(
        &self,
        id: &str,
        path: &str,
    ) -> Result<Project, AowError> {
        let _operation = self.inner.project_operation.lock().await;
        self.lock()?
            .projects
            .iter()
            .any(|project| project.id == id)
            .then_some(())
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        let notes_path = notes::prepare_notes_directory(Path::new(path)).await?;
        let item = {
            let mut state = self.lock()?;
            let index = state
                .projects
                .iter()
                .position(|project| project.id == id)
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let previous = state.projects[index].clone();
            state.projects[index].notes_path = notes_path;
            state.projects[index].notes_identity = None;
            state.projects[index].notes_custom = true;
            let item = state.projects[index].clone();
            if let Err(error) = self.persist_projects(&state.projects) {
                state.projects[index] = previous;
                return Err(error);
            }
            item
        };
        project_from_stored_strict(item).await
    }

    pub(in crate::aow) async fn remove_project(&self, id: &str) -> Result<(), AowError> {
        let _operation = self.inner.project_operation.lock().await;
        let mut state = self.lock()?;
        if self.inner.removals.project_busy(id)? {
            return Err(AowError::RemovalConflict(
                "Project 正在清理 Worktree，请等待完成".into(),
            ));
        }
        let index = state
            .projects
            .iter()
            .position(|project| project.id == id)
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        if state.projects[index].builtin {
            return Err(AowError::Invalid("内置浮动工作区不能移除".into()));
        }
        let removed = state.projects.remove(index);
        if let Err(error) = self.persist_projects(&state.projects) {
            state.projects.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::aow) async fn project(&self, id: &str) -> Result<Project, AowError> {
        let stored = self
            .lock()?
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
            .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
        project_from_stored_strict(stored).await
    }
}
