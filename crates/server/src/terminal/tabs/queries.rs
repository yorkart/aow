use super::*;

impl TerminalManager {
    pub(in crate::terminal) fn list_snapshot(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<TerminalTabList, TerminalError> {
        let state = self.lock_state()?;
        let tabs = state
            .tabs
            .iter()
            .filter(|tab| {
                workspace_root
                    .map(|root| tab.workspace_root == root)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        Ok(TerminalTabList { tabs })
    }

    pub(in crate::terminal) fn get_snapshot(
        &self,
        tab_id: &str,
    ) -> Result<TerminalTab, TerminalError> {
        self.lock_state()?
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))
    }

    pub(in crate::terminal) async fn list(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<TerminalTabList, TerminalError> {
        self.reconcile().await?;
        self.list_snapshot(workspace_root)
    }

    pub(in crate::terminal) async fn agents(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<TerminalAgentList, TerminalError> {
        let mut detected = self
            .inner
            .terminald
            .agents()
            .await
            .map_err(map_client_error)?;
        let tabs = self.list_snapshot(workspace_root)?.tabs;
        let panes: HashSet<_> = tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .map(|pane| &pane.id)
            .collect();
        detected.agents.retain(|id, _| panes.contains(id));
        detected.titles.retain(|id, _| panes.contains(id));
        detected.processes.retain(|id, _| panes.contains(id));
        super::super::pi_detection::enrich(&self.inner.terminald, &tabs, &mut detected).await;
        session_titles::enrich(&mut detected).await;
        Ok(detected)
    }

    pub(in crate::terminal) async fn get(
        &self,
        tab_id: &str,
    ) -> Result<TerminalTab, TerminalError> {
        self.reconcile().await?;
        self.get_snapshot(tab_id)
    }
}
