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
}

#[cfg(test)]
mod tests;
