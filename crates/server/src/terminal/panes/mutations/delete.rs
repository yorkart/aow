use super::*;

impl TerminalManager {
    pub(in crate::terminal) async fn delete_pane(
        &self,
        tab_id: &str,
        pane_id: &str,
    ) -> Result<Option<TerminalTab>, TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let result = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            if !state.tabs[index]
                .panes
                .iter()
                .any(|pane| pane.id == pane_id)
            {
                return Err(TerminalError::PaneNotFound(pane_id.to_owned()));
            }
            let old = state.tabs[index].clone();
            let result = if old.panes.len() == 1 {
                state.tabs.remove(index);
                None
            } else {
                state.tabs[index].layout = remove_layout_leaf(old.layout.clone(), pane_id)
                    .ok_or_else(|| TerminalError::Invalid("layout cannot be empty".to_owned()))?;
                state.tabs[index].panes.retain(|pane| pane.id != pane_id);
                touch_tab(&mut state.tabs[index]);
                Some(state.tabs[index].clone())
            };
            if let Err(error) = self.persist_locked(&state) {
                if result.is_none() {
                    state.tabs.insert(index, old);
                } else {
                    state.tabs[index] = old;
                }
                return Err(error);
            }
            result
        };
        self.inner
            .terminald
            .delete(pane_id)
            .await
            .map_err(map_client_error)?;
        Ok(result)
    }
}
