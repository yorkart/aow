//! Fixture setup only. Business paths always use the external Runtime.
use crate::error::ServiceError;
use std::path::Path;
pub(super) async fn git_output(cwd: &Path, args: &[&str]) -> Result<String, ServiceError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        return Err(ServiceError::Git(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| ServiceError::Git(e.to_string()))
}

pub(crate) type TestServices = crate::AppState;
impl crate::AppState {
    pub(crate) fn in_memory() -> Self {
        Self::for_test(Default::default())
    }
    pub(crate) fn persistent(
        path: &std::path::Path,
    ) -> Result<Self, crate::terminal::TerminalError> {
        Self::for_test_with_state_dir(Default::default(), path.to_owned(), Default::default())
    }
    pub(crate) fn persistent_with_notes_base(
        path: &std::path::Path,
        notes: std::path::PathBuf,
    ) -> Result<Self, crate::terminal::TerminalError> {
        let state = Self::persistent(path)?;
        if !state
            .settings
            .test_settings_path()
            .is_some_and(|path| path.exists())
        {
            state.settings.test_settings().unwrap().notes_base =
                notes.to_string_lossy().into_owned();
        }
        Ok(state)
    }
    pub(crate) fn replace_services(&mut self, other: Self) {
        self.projects = other.projects;
        self.agents = other.agents;
        self.roles = other.roles;
        self.tasks = other.tasks;
        self.settings = other.settings;
        self.node = other.node;
        self.notifications = other.notifications;
        self.cli_access = other.cli_access;
    }
}
