use super::*;
impl AowManager {
    pub(in crate::aow) async fn project_from_stored(&self, stored: StoredProject) -> Project {
        let fallback = Project {
            id: stored.id.clone(),
            name: stored.name.clone(),
            registered_path: stored.registered_path.clone(),
            common_git_dir: String::new(),
            notes_path: stored.notes_path.clone(),
            builtin: stored.builtin,
            avatar_url: stored.avatar_url.clone(),
            worktrees: Vec::new(),
            task_worktree: stored.task_worktree.clone(),
            error: None,
        };
        match self.project_from_stored_strict(stored).await {
            Ok(project) => project,
            Err(error) => Project {
                error: Some(error.to_string()),
                ..fallback
            },
        }
    }

    pub(in crate::aow) async fn project_from_stored_strict(
        &self,
        stored: StoredProject,
    ) -> Result<Project, AowError> {
        let registered_path = PathBuf::from(&stored.registered_path);
        let common = self.resolve_common_git_dir(&registered_path).await?;
        let output = self
            .git_output(&registered_path, &["worktree", "list", "--porcelain"])
            .await?;
        let mut worktrees = super::worktrees::parse_worktrees(&stored.id, &output);
        for worktree in &mut worktrees {
            worktree.color = stored
                .worktree_colors
                .get(&worktree.path)
                .copied()
                .unwrap_or_default();
            worktree.icon = stored
                .worktree_icons
                .get(&worktree.path)
                .copied()
                .unwrap_or_default();
        }
        Ok(Project {
            id: stored.id,
            name: stored.name,
            registered_path: stored.registered_path,
            common_git_dir: common,
            notes_path: stored.notes_path,
            builtin: stored.builtin,
            avatar_url: stored.avatar_url,
            worktrees,
            task_worktree: stored.task_worktree,
            error: None,
        })
    }

    pub(in crate::aow) async fn resolve_common_git_dir(
        &self,
        repository: &Path,
    ) -> Result<String, AowError> {
        let common = self
            .git_output(repository, &["rev-parse", "--git-common-dir"])
            .await?;
        let common = PathBuf::from(common.trim());
        let common = if common.is_absolute() {
            common
        } else {
            repository.join(common)
        };
        Ok(self
            .canonical_directory(&common)
            .await?
            .to_string_lossy()
            .into_owned())
    }
}
