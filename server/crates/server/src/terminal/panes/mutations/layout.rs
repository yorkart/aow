use super::*;

impl TerminalManager {
    pub(in crate::terminal) fn update_layout(
        &self,
        tab_id: &str,
        request: UpdateLayoutRequest,
    ) -> Result<TerminalTab, TerminalError> {
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        if request
            .revision
            .is_some_and(|revision| revision != state.tabs[index].revision)
        {
            return Err(TerminalError::Conflict(format!(
                "layout revision is stale; current revision is {}",
                state.tabs[index].revision
            )));
        }
        validate_layout(&request.layout, &state.tabs[index].panes)?;
        let old = state.tabs[index].clone();
        state.tabs[index].layout = request.layout;
        touch_tab(&mut state.tabs[index]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = old;
            return Err(error);
        }
        Ok(state.tabs[index].clone())
    }
}
