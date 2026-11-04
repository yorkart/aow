use super::*;

const FLOATING_PROJECT_ID: &str = "__aow_floating";

impl AowManager {
    pub(crate) async fn initialize_global(&self) -> Result<(), AowError> {
        let Some(state_directory) = &self.inner.state_dir else {
            return Ok(());
        };
        let _operation = self.inner.project_operation.lock().await;
        let node_root =
            if let Some(runtime) = self.node() {
                let value: serde_json::Value = runtime
                    .get_json("/v1/environment")
                    .await
                    .map_err(super::workspace::node_error)?;
                PathBuf::from(value["state_dir"].as_str().ok_or_else(|| {
                    AowError::Invalid("Node state directory is unavailable".into())
                })?)
            } else {
                state_directory.clone()
            };
        if self.node().is_some()
            && self
                .inner
                .settings_path
                .as_ref()
                .is_some_and(|path| !path.exists())
        {
            let mut settings = self.lock_settings()?;
            settings.notes_base = node_root.join("data/notes").to_string_lossy().into_owned();
            self.persist_settings(&settings)?;
        }
        let directory = node_root.join("data/repos").join(FLOATING_PROJECT_ID);
        self.node_mkdir(&directory).await?;
        if !self
            .node_metadata(&directory.join(".git"), true)
            .await?
            .exists
        {
            // Keep initialization compatible with Git versions before 2.28.
            self.git_output(&directory, &["init"]).await?;
            self.git_output(&directory, &["symbolic-ref", "HEAD", "refs/heads/main"])
                .await?;
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
        project.name = aow_i18n::text("server.floating_workspace").into();
        project.builtin = true;
        // There is one floating builtin project. Restored records can have a
        // different identity or another machine's repository path.
        state
            .projects
            .retain(|project| !project.builtin && project.id != FLOATING_PROJECT_ID);
        state.projects.insert(0, project);
        self.persist_projects(&state.projects)
    }
}

#[cfg(test)]
mod tests;
