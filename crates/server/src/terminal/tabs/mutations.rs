use super::*;

impl TerminalManager {
    pub(in crate::terminal) fn update(
        &self,
        tab_id: &str,
        request: UpdateTerminalRequest,
    ) -> Result<TerminalTab, TerminalError> {
        if request.name.is_none() {
            return Err(TerminalError::Invalid(
                "terminal update must include name".to_owned(),
            ));
        }
        let name = request.name.as_deref().map(validate_name).transpose()?;
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        if state.tabs[index]
            .panes
            .iter()
            .any(|pane| pane.agent_terminal.is_some())
        {
            return Err(TerminalError::Invalid(
                "CLI-created terminal tabs cannot be renamed".to_owned(),
            ));
        }
        let old = state.tabs[index].clone();
        if let Some(name) = name {
            state.tabs[index].name = name;
            state.tabs[index].name_is_custom = Some(true);
        }
        touch_tab(&mut state.tabs[index]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = old;
            return Err(error);
        }
        Ok(state.tabs[index].clone())
    }

    pub(in crate::terminal) fn reorder(
        &self,
        request: ReorderTerminalsRequest,
    ) -> Result<TerminalTabList, TerminalError> {
        if request.workspace_root.trim().is_empty() {
            return Err(TerminalError::Invalid(
                "workspace_root cannot be empty".to_owned(),
            ));
        }
        let requested_ids = request.tab_ids.iter().collect::<HashSet<_>>();
        if requested_ids.len() != request.tab_ids.len() {
            return Err(TerminalError::Invalid(
                "terminal order contains duplicate tab ids".to_owned(),
            ));
        }

        let mut state = self.lock_state()?;
        let workspace_indices = state
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                (tab.workspace_root == request.workspace_root).then_some(index)
            })
            .collect::<Vec<_>>();
        let current_ids = workspace_indices
            .iter()
            .map(|index| state.tabs[*index].id.as_str())
            .collect::<HashSet<_>>();
        if requested_ids.len() != current_ids.len()
            || !requested_ids
                .iter()
                .all(|tab_id| current_ids.contains(tab_id.as_str()))
        {
            return Err(TerminalError::Conflict(
                "terminal order is stale; refresh the terminal list and try again".to_owned(),
            ));
        }

        let requested_order = request
            .tab_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let current_order = workspace_indices
            .iter()
            .map(|index| state.tabs[*index].id.as_str())
            .collect::<Vec<_>>();
        if current_order == requested_order {
            return Ok(TerminalTabList {
                tabs: workspace_indices
                    .iter()
                    .map(|index| state.tabs[*index].clone())
                    .collect(),
            });
        }

        let reordered = state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_root == request.workspace_root)
            .map(|tab| (tab.id.clone(), tab.clone()))
            .collect::<HashMap<_, _>>();
        let old = state.tabs.clone();
        for (index, tab_id) in workspace_indices.iter().zip(&request.tab_ids) {
            state.tabs[*index] = reordered
                .get(tab_id)
                .expect("validated terminal order must contain every tab")
                .clone();
        }
        if let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(TerminalTabList {
            tabs: workspace_indices
                .iter()
                .map(|index| state.tabs[*index].clone())
                .collect(),
        })
    }

    pub(in crate::terminal) async fn delete_tab(&self, tab_id: &str) -> Result<(), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let pane_ids = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            let tab = state.tabs.remove(index);
            if let Err(error) = self.persist_locked(&state) {
                state.tabs.insert(index, tab);
                return Err(error);
            }
            tab.panes
                .into_iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>()
        };
        for pane_id in pane_ids {
            self.inner
                .terminald
                .delete(&pane_id)
                .await
                .map_err(map_client_error)?;
        }
        Ok(())
    }
}
