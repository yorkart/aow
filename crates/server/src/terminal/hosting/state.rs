use super::*;

impl TerminalManager {
    pub(in crate::terminal) fn hosting_gate(
        &self,
        pane_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, TerminalError> {
        let mut gates = self
            .inner
            .hosting_gates
            .lock()
            .map_err(|_| TerminalError::Poisoned)?;
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(pane_id).and_then(std::sync::Weak::upgrade) {
            return Ok(gate);
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        gates.insert(pane_id.into(), Arc::downgrade(&gate));
        Ok(gate)
    }

    pub(in crate::terminal) fn hosting(
        &self,
        pane_id: &str,
    ) -> Result<Option<TerminalHosting>, TerminalError> {
        let state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        Ok(state.tabs[tab].panes[pane].hosting.clone())
    }

    /// Workers supply their generation for atomic conditional updates, including
    /// failures. User changes and terminal input also serialize with hosting_gate;
    /// expected=None is reserved for explicit user actions.
    pub(in crate::terminal) fn set_hosting(
        &self,
        pane_id: &str,
        expected: Option<&str>,
        hosting: Option<TerminalHosting>,
    ) -> Result<bool, TerminalError> {
        let mut state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        if expected.is_some_and(|id| {
            state.tabs[tab].panes[pane]
                .hosting
                .as_ref()
                .is_none_or(|current| current.id != id)
        }) {
            return Ok(false);
        }
        let old = state.tabs[tab].clone();
        state.tabs[tab].panes[pane].hosting = hosting;
        touch_tab(&mut state.tabs[tab]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[tab] = old;
            return Err(error);
        }
        Ok(true)
    }
}

pub(super) fn transition(hosting: &mut TerminalHosting, phase: TerminalHostingPhase) {
    hosting.phase = phase;
    hosting.phase_started_at = timestamp();
}

pub(super) fn elapsed(hosting: &TerminalHosting) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&hosting.phase_started_at).map_or(i64::MAX, |started| {
        (chrono::Utc::now() - started.to_utc()).num_seconds()
    })
}
