use super::*;
impl ProjectService {
    pub(crate) async fn execution_reference_path(
        &self,
        project_id: &str,
        config: &aow_host_contract::workspace::WorkspaceConfig,
    ) -> Result<String, ServiceError> {
        config
            .validate()
            .map_err(|error| ServiceError::Invalid(aow_i18n::text(error.0).into()))?;
        if config.workspace_mode == aow_host_contract::workspace::WorkspaceMode::Existing {
            return self
                .agent_worktree_path(project_id, &config.workspace_path.to_string_lossy())
                .await;
        }
        let project = self.project(project_id).await?;
        let main = project
            .worktrees
            .iter()
            .find(|tree| tree.is_main)
            .or_else(|| project.worktrees.first())
            .ok_or_else(|| {
                ServiceError::Invalid(
                    aow_i18n::text("server.project_has_no_available_worktrees").into(),
                )
            })?;
        self.agent_worktree_path(project_id, &main.path).await
    }
    pub(crate) async fn automation_project_for_cwd(
        &self,
        id: &str,
        cwd: &Path,
    ) -> Result<(String, PathBuf), ServiceError> {
        let cwd = self.node.canonical_directory(cwd).await?;
        // Resolve Git ownership rather than accepting any path beneath a registered
        // root: a subdirectory may itself be a different repository or submodule.
        let root = self
            .node
            .git_output(&cwd, &["rev-parse", "--show-toplevel"])
            .await?;
        self.automation_project(id, Path::new(root.trim_end()))
            .await
    }
    pub(crate) async fn automation_project(
        &self,
        id: &str,
        worktree_path: &std::path::Path,
    ) -> Result<(String, PathBuf), ServiceError> {
        let project = self.project(id).await?;
        let requested = self.node.canonical_directory(worktree_path).await?;
        if project.error.is_some()
            || !project
                .worktrees
                .iter()
                .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(ServiceError::Invalid(
                aow_i18n::text("server.workspace_does_not_belong_to_the_selected_project").into(),
            ));
        }
        Ok((project.name, PathBuf::from(project.registered_path)))
    }
    pub(crate) async fn agent_worktree_path(
        &self,
        project_id: &str,
        worktree_path: &str,
    ) -> Result<String, ServiceError> {
        let project = self.project(project_id).await?;
        let requested = self
            .node
            .canonical_directory(Path::new(worktree_path))
            .await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(ServiceError::Invalid(format!(
                "agent cwd is not a worktree of project {project_id}: {}",
                requested.display()
            )));
        }
        Ok(requested.to_string_lossy().into_owned())
    }
}
