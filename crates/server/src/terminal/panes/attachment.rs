use super::*;

impl TerminalManager {
    pub(in crate::terminal) fn ensure_pane(
        &self,
        tab_id: &str,
        pane_id: &str,
    ) -> Result<(), TerminalError> {
        let state = self.lock_state()?;
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
        if tab.panes.iter().any(|pane| pane.id == pane_id) {
            Ok(())
        } else {
            Err(TerminalError::PaneNotFound(pane_id.to_owned()))
        }
    }

    pub(in crate::terminal) async fn attach(
        &self,
        tab_id: &str,
        pane_id: &str,
        options: AttachOptions<'_>,
    ) -> Result<TerminaldAttachStream, TerminalError> {
        self.ensure_pane(tab_id, pane_id)?;
        let hosted = self.hosting(pane_id)?.is_some();
        let controlled = options.controlled || self.cli_agent(pane_id).is_ok() || hosted;
        let mut daemon_options = aow_terminald_client::TerminalAttachOptions {
            epoch: options.epoch,
            after: options.after,
            controlled,
            vt_snapshot: options.vt_snapshot,
            observer: options.observer || hosted,
            gzip_snapshot: options.gzip_snapshot,
        };
        let attachment = self
            .inner
            .terminald
            .attach_with_options(pane_id, daemon_options)
            .await;
        match attachment {
            Ok(socket) => return Ok(socket),
            Err(error) if client_error_is_not_found(&error) => {
                // The daemon can restart between page load and WS upgrade.
                self.reconcile().await?;
            }
            Err(error) => return Err(map_client_error(error)),
        }
        // Reconcile may have recreated this pane as a new runtime. Never reuse
        // a cursor from the previous terminald instance, but preserve the
        // browser's requested control protocol across the retry.
        daemon_options.epoch = None;
        daemon_options.after = None;
        daemon_options.observer = options.observer || self.hosting(pane_id)?.is_some();
        let attachment = self
            .inner
            .terminald
            .attach_with_options(pane_id, daemon_options)
            .await;
        attachment.map_err(|error| {
            if client_error_is_not_found(&error) {
                TerminalError::DaemonUnavailable(
                    "terminal runtime disappeared while attaching".to_owned(),
                )
            } else {
                map_client_error(error)
            }
        })
    }

    pub(in crate::terminal) fn apply_attach_status(
        &self,
        pane_id: &str,
        status: TerminalPaneStatus,
        exit_code: Option<u32>,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let Some((tab_index, pane_index)) = find_pane(&state.tabs, pane_id) else {
            return Ok(());
        };
        let pane = &state.tabs[tab_index].panes[pane_index];
        if pane.status == status && pane.exit_code == exit_code {
            return Ok(());
        }
        let old = state.tabs[tab_index].clone();
        let now = timestamp();
        let pane = &mut state.tabs[tab_index].panes[pane_index];
        pane.status = status;
        pane.exit_code = exit_code;
        pane.updated_at.clone_from(&now);
        state.tabs[tab_index].updated_at = now;
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[tab_index] = old;
            return Err(error);
        }
        Ok(())
    }
}
