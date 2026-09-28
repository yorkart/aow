use super::*;

mod agent;
mod terminal;

impl TerminalManager {
    pub(in crate::terminal) async fn runtime_matches(
        &self,
        pane_id: &str,
        expected: &TerminalRuntimeSpec,
    ) -> Result<bool, TerminalError> {
        match self.inner.terminald.get(pane_id).await {
            Ok(Some(runtime)) => Ok(runtime.spec() == *expected),
            Ok(None) => Ok(false),
            Err(error) => {
                tracing::debug!(%error, %pane_id, "could not probe ambiguous terminal create result");
                Ok(false)
            }
        }
    }

    pub(in crate::terminal) async fn delete_failed_runtime_best_effort(&self, pane_id: &str) {
        if let Err(error) = self.inner.terminald.delete(pane_id).await {
            // Once rollback removes this ID from desired JSON, a later
            // successful reconcile also recognizes it as an orphan.
            tracing::warn!(%error, %pane_id, "failed to clean up ambiguous terminal create");
        }
    }

    fn rollback_created_tab(&self, tab_id: &str) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let Some(index) = state.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Ok(());
        };
        let tab = state.tabs.remove(index);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs.insert(index, tab);
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::terminal) fn runtime_spec(&self, pane: &TerminalPane) -> TerminalRuntimeSpec {
        runtime_spec(pane)
    }

    fn ensure_creation_available(
        &self,
        request: &CreateTerminalRequest,
    ) -> Result<(), TerminalError> {
        let state = self.lock_state()?;
        if let Some(root) = &request.workspace_root {
            state.ensure_workspace_available(root)?;
        }
        if let Some(cwd) = &request.cwd {
            state.ensure_workspace_available(cwd)?;
        }
        Ok(())
    }
}
