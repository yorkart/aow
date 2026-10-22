use super::*;

#[derive(Deserialize)]
pub(super) struct OpenProjectDockRequest {
    /// Explicit association saved by the browser before server-owned docks existed.
    legacy_terminal_id: Option<String>,
    workspace_root: Option<String>,
}

pub(super) async fn open(
    State(state): State<AppState>,
    AxumPath(project_id): AxumPath<String>,
    Json(request): Json<OpenProjectDockRequest>,
) -> Result<Json<TerminalTab>, HttpError> {
    let (root, name) = state
        .aow
        .project_terminal_defaults(&project_id, request.workspace_root.as_deref())
        .await
        .map_err(crate::aow::aow_http_error)?;
    state
        .terminals
        .project_dock(
            &project_id,
            &root,
            &name,
            request.legacy_terminal_id.as_deref(),
        )
        .await
        .map(Json)
        .map_err(terminal_http_error)
}

impl TerminalManager {
    pub(in crate::terminal) async fn project_dock(
        &self,
        project_id: &str,
        root: &str,
        name: &str,
        legacy_id: Option<&str>,
    ) -> Result<TerminalTab, TerminalError> {
        self.create_shell(
            CreateTerminalRequest {
                name: Some(name.to_owned()),
                cwd: Some(root.to_owned()),
                workspace_root: Some(root.to_owned()),
                shell: None,
                agent_id: None,
                resume_session_id: None,
                rows: None,
                cols: None,
            },
            Some((project_id, legacy_id)),
        )
        .await
    }

    /// Called under the operation lock, before allocating a new shell.
    pub(in crate::terminal) fn existing_project_dock(
        &self,
        project_id: &str,
        root: &str,
        legacy_id: Option<&str>,
    ) -> Result<Option<TerminalTab>, TerminalError> {
        let mut state = self.lock_state()?;
        if let Some(tab) = state
            .tabs
            .iter()
            .find(|tab| {
                tab.dock_project_id.as_deref() == Some(project_id) && tab.workspace_root == root
            })
        {
            return Ok(Some(tab.clone()));
        }
        let Some(index) = legacy_id.and_then(|id| state.tabs.iter().position(|tab| tab.id == id))
        else {
            return Ok(None);
        };
        let old = state.tabs[index].clone();
        if old.workspace_root != root
            || old.dock_project_id.is_some()
            || old.panes.iter().any(|pane| {
                pane.kind != TerminalPaneKind::Terminal || pane.agent_terminal.is_some()
            })
        {
            return Err(TerminalError::Invalid(
                "legacy project terminal does not belong to this project".into(),
            ));
        }
        state.tabs[index].dock_project_id = Some(project_id.to_owned());
        touch_tab(&mut state.tabs[index]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = old;
            return Err(error);
        }
        Ok(Some(state.tabs[index].clone()))
    }
}

#[cfg(test)]
mod tests;
