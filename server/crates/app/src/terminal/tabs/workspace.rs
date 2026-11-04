use super::*;

impl TerminalManager {
    pub(crate) fn workspace_tab_counts(
        &self,
        workspace_root: &str,
    ) -> Result<(usize, usize), TerminalError> {
        let state = self.lock_state()?;
        let matching = state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_root == workspace_root);
        let mut terminal_tabs = 0;
        let mut agent_tabs = 0;
        for tab in matching {
            if tab
                .panes
                .iter()
                .any(|pane| pane.kind == TerminalPaneKind::Agent)
            {
                agent_tabs += 1;
            } else {
                terminal_tabs += 1;
            }
        }
        Ok((terminal_tabs, agent_tabs))
    }

    pub(crate) fn set_workspace_removing(
        &self,
        workspace_root: &str,
        removing: bool,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        if removing {
            state.removing_workspaces.insert(workspace_root.to_owned());
        } else {
            state.removing_workspaces.remove(workspace_root);
        }
        Ok(())
    }

    pub(crate) async fn delete_workspace(
        &self,
        workspace_root: &str,
    ) -> Result<(usize, usize), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let (tab_ids, pane_ids, terminal_tabs, agent_tabs) = {
            let state = self.lock_state()?;
            let tabs = state
                .tabs
                .iter()
                .filter(|tab| tab.workspace_root == workspace_root)
                .cloned()
                .collect::<Vec<_>>();
            let agent_tabs = tabs
                .iter()
                .filter(|tab| {
                    tab.panes
                        .iter()
                        .any(|pane| pane.kind == TerminalPaneKind::Agent)
                })
                .count();
            let tab_ids = tabs
                .iter()
                .map(|tab| tab.id.clone())
                .collect::<HashSet<_>>();
            let pane_ids = tabs
                .iter()
                .flat_map(|tab| tab.panes.iter().map(|pane| pane.id.clone()))
                .collect::<Vec<_>>();
            let terminal_tabs = tabs.len() - agent_tabs;
            (tab_ids, pane_ids, terminal_tabs, agent_tabs)
        };
        if tab_ids.is_empty() {
            return Ok((0, 0));
        }

        // Stop and reap every runtime before removing durable desired state.
        // If a daemon call fails, the worktree removal is aborted and durable
        // metadata remains available for an explicit retry.
        for pane_id in pane_ids {
            self.inner
                .terminald
                .delete(&pane_id)
                .await
                .map_err(map_client_error)?;
        }

        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        state.tabs.retain(|tab| !tab_ids.contains(&tab.id));
        if let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok((terminal_tabs, agent_tabs))
    }
}
