use super::*;
impl ProjectService {
    pub(crate) async fn registered_worktree_path(
        &self,
        worktree_path: &str,
    ) -> Result<PathBuf, ServiceError> {
        if let Some(runtime) = self.node.node() {
            return runtime
                .resolve_native_worktree(Path::new(worktree_path), &self.repository_roots()?)
                .await
                .map_err(|error| ServiceError::Invalid(error.to_string()));
        }
        let requested = self
            .node
            .canonical_directory(Path::new(worktree_path))
            .await?;
        let projects = self.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(ServiceError::Invalid(format!(
                "session cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        Ok(requested)
    }
}
