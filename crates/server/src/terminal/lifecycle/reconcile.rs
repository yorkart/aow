use super::super::*;

impl TerminalManager {
    pub(crate) async fn reconcile(&self) -> Result<(), TerminalError> {
        let _operation = self.inner.operation.lock().await;
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let before = self
                .inner
                .terminald
                .health()
                .await
                .map_err(map_client_error)?;
            let runtimes = self
                .inner
                .terminald
                .list()
                .await
                .map_err(map_client_error)?;
            let desired = {
                let state = self.lock_state()?;
                state
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.panes.iter().cloned())
                    .map(|pane| (pane.id.clone(), pane))
                    .collect::<HashMap<_, _>>()
            };
            let actual = runtimes
                .into_iter()
                .map(|runtime| (runtime.id.clone(), runtime))
                .collect::<HashMap<_, _>>();

            // The JSON document is the desired runtime set. Delete daemon-only
            // instances first so a prior control-plane crash cannot leak shells.
            for runtime_id in actual.keys().filter(|id| !desired.contains_key(*id)) {
                self.inner
                    .terminald
                    .delete(runtime_id)
                    .await
                    .map_err(map_client_error)?;
            }

            // Existing runtimes are deliberately preserved even if their live
            // size differs. Web restarts must not replace a healthy shell.
            // Every persisted pane is desired state (including one last seen
            // exited/interrupted), so daemon restart recreates all missing
            // panes as fresh running shells.
            for pane in desired.values() {
                if !actual.contains_key(&pane.id) && pane.restart_on_daemon_restart {
                    self.inner
                        .terminald
                        .create(&pane.id, &self.runtime_spec(pane))
                        .await
                        .map_err(map_client_error)?;
                }
            }
            self.mark_missing_nonrestart_panes(&desired, &actual)?;

            let mut reconciled = HashMap::new();
            for id in desired.keys() {
                if let Some(runtime) = self
                    .inner
                    .terminald
                    .get(id)
                    .await
                    .map_err(map_client_error)?
                {
                    reconciled.insert(id.clone(), runtime);
                }
            }
            let after = self
                .inner
                .terminald
                .health()
                .await
                .map_err(map_client_error)?;
            if before.instance_id != after.instance_id {
                tracing::warn!(
                    attempt,
                    before_instance_id = %before.instance_id,
                    after_instance_id = %after.instance_id,
                    "terminald restarted during reconciliation; retrying"
                );
                continue;
            }

            self.record_daemon_instance(after.instance_id)?;
            return self.apply_runtime_statuses(&reconciled);
        }
        Err(TerminalError::DaemonUnavailable(
            "terminald restarted repeatedly during reconciliation".to_owned(),
        ))
    }

    fn record_daemon_instance(&self, instance_id: String) -> Result<(), TerminalError> {
        let mut instance = self
            .inner
            .daemon_instance_id
            .lock()
            .map_err(|_| TerminalError::Poisoned)?;
        let previous = instance.replace(instance_id.clone());
        if previous.as_deref().is_some_and(|id| id != instance_id) {
            tracing::info!(
                previous_instance_id = ?previous,
                %instance_id,
                "terminald instance changed; desired runtimes were rebuilt"
            );
        }
        Ok(())
    }

    fn apply_runtime_statuses(
        &self,
        runtimes: &HashMap<String, TerminalRuntime>,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        let mut changed = false;
        for tab in &mut state.tabs {
            let mut tab_changed = false;
            for pane in &mut tab.panes {
                let Some(runtime) = runtimes.get(&pane.id) else {
                    continue;
                };
                if apply_runtime(pane, runtime) {
                    tab_changed = true;
                    changed = true;
                }
            }
            if tab_changed {
                // Runtime state and size are not structural layout changes.
                tab.updated_at = timestamp();
            }
        }
        if changed && let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(())
    }

    fn mark_missing_nonrestart_panes(
        &self,
        desired: &HashMap<String, TerminalPane>,
        actual: &HashMap<String, TerminalRuntime>,
    ) -> Result<(), TerminalError> {
        let missing = desired
            .values()
            .filter(|pane| !pane.restart_on_daemon_restart && !actual.contains_key(&pane.id))
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let mut state = self.lock_state()?;
        let old = state.tabs.clone();
        let now = timestamp();
        let mut changed = false;
        for tab in &mut state.tabs {
            let mut tab_changed = false;
            for pane in &mut tab.panes {
                if missing.contains(pane.id.as_str()) && pane.status == TerminalPaneStatus::Running
                {
                    pane.status = TerminalPaneStatus::Interrupted;
                    pane.exit_code = None;
                    pane.updated_at = now.clone();
                    tab_changed = true;
                    changed = true;
                }
            }
            if tab_changed {
                tab.updated_at = now.clone();
            }
        }
        if changed && let Err(error) = self.persist_locked(&state) {
            state.tabs = old;
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::terminal) async fn sync_runtime(
        &self,
        pane_id: &str,
    ) -> Result<(), TerminalError> {
        // Multiple browser bridges for one pane can overlap briefly during a
        // takeover. Serialize get+apply so every later ACK observes and stores
        // terminald's latest authoritative state rather than a stale snapshot.
        let _sync = self.inner.runtime_sync.lock().await;
        let runtime = self
            .inner
            .terminald
            .get(pane_id)
            .await
            .map_err(map_client_error)?;
        if let Some(runtime) = runtime {
            self.apply_runtime_statuses(&HashMap::from([(pane_id.to_owned(), runtime)]))?;
        }
        Ok(())
    }
}
