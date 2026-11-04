use std::sync::MutexGuard;

use super::super::*;

impl TerminalManager {
    pub(crate) fn in_memory(terminald: TerminalClient) -> Self {
        #[cfg(test)]
        let clipboard = clipboard::ClipboardStorage::temporary();
        let manager = Self {
            inner: Arc::new(ManagerInner {
                #[cfg(test)]
                node_workspace: std::sync::atomic::AtomicBool::new(false),
                metadata_path: None,
                state: Mutex::new(ManagerState::default()),
                terminald,
                daemon_instance_id: Mutex::new(None),
                operation: tokio::sync::Mutex::new(()),
                runtime_sync: tokio::sync::Mutex::new(()),
                restored_initializations: Mutex::new(HashSet::new()),
                agent_operations: Mutex::new(HashMap::new()),
                hosting_gates: Mutex::new(HashMap::new()),
                hosting_workers: Mutex::new(HashSet::new()),
                #[cfg(test)]
                clipboard,
                #[cfg(test)]
                clipboard_operation: tokio::sync::Mutex::new(()),
                task_stops: tokio::sync::broadcast::channel(128).0,
                task_completions: tokio::sync::broadcast::channel(128).0,
                notifications_started: std::sync::atomic::AtomicBool::new(false),
                sync: sync::TerminalSync::default(),
            }),
        };
        #[cfg(test)]
        manager.start_clipboard_cleanup();
        manager
    }

    pub(crate) fn persistent(
        state_dir: PathBuf,
        terminald: TerminalClient,
    ) -> Result<Self, TerminalError> {
        std::fs::create_dir_all(state_dir.join("state"))?;
        #[cfg(test)]
        let clipboard = clipboard::ClipboardStorage::persistent(&state_dir)?;
        let metadata_path = state_dir.join("state").join(METADATA_FILE);
        let mut tabs = match std::fs::read(&metadata_path) {
            Ok(bytes) => {
                let persisted: PersistedState = serde_json::from_slice(&bytes)?;
                if persisted.version != METADATA_VERSION {
                    return Err(TerminalError::Invalid(format!(
                        "unsupported terminal metadata version {}",
                        persisted.version
                    )));
                }
                persisted.tabs
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        for tab in &mut tabs {
            for pane in &mut tab.panes {
                // Pane names are generated. Legacy custom labels are discarded.
                pane.name = default_pane_name(&pane.cwd, &pane.shell);
            }
            migrate_terminal_names(tab);
        }
        validate_persisted_tabs(&tabs)?;
        let restored_initializations = tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .filter(|pane| {
                pane.agent_terminal
                    .as_ref()
                    .is_some_and(|state| state.phase == aow_protocol::AgentTerminalPhase::Starting)
            })
            .map(|pane| pane.id.clone())
            .collect();
        let manager = Self {
            inner: Arc::new(ManagerInner {
                #[cfg(test)]
                node_workspace: std::sync::atomic::AtomicBool::new(false),
                metadata_path: Some(metadata_path),
                state: Mutex::new(ManagerState {
                    tabs,
                    ..ManagerState::default()
                }),
                terminald,
                daemon_instance_id: Mutex::new(None),
                operation: tokio::sync::Mutex::new(()),
                runtime_sync: tokio::sync::Mutex::new(()),
                restored_initializations: Mutex::new(restored_initializations),
                agent_operations: Mutex::new(HashMap::new()),
                hosting_gates: Mutex::new(HashMap::new()),
                hosting_workers: Mutex::new(HashSet::new()),
                #[cfg(test)]
                clipboard,
                #[cfg(test)]
                clipboard_operation: tokio::sync::Mutex::new(()),
                task_stops: tokio::sync::broadcast::channel(128).0,
                task_completions: tokio::sync::broadcast::channel(128).0,
                notifications_started: std::sync::atomic::AtomicBool::new(false),
                sync: sync::TerminalSync::default(),
            }),
        };
        #[cfg(test)]
        manager.start_clipboard_cleanup();
        Ok(manager)
    }

    #[cfg(test)]
    fn start_clipboard_cleanup(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let inner = Arc::downgrade(&self.inner);
        handle.spawn(async move {
            let mut interval = tokio::time::interval(CLIPBOARD_CLEANUP_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(inner) = inner.upgrade() else {
                    break;
                };
                let _operation = inner.clipboard_operation.lock().await;
                if let Err(error) = inner.clipboard.clean_expired() {
                    tracing::warn!(%error, "failed to clean expired clipboard images");
                }
            }
        });
    }

    pub(in crate::terminal) fn lock_state(
        &self,
    ) -> Result<MutexGuard<'_, ManagerState>, TerminalError> {
        self.inner.state.lock().map_err(|_| TerminalError::Poisoned)
    }

    pub(in crate::terminal) fn persist_locked(
        &self,
        state: &ManagerState,
    ) -> Result<(), TerminalError> {
        if let Some(path) = &self.inner.metadata_path {
            atomic_save(
                path,
                &PersistedState {
                    version: METADATA_VERSION,
                    tabs: state.tabs.clone(),
                },
            )?;
        }
        // Publish only committed state, while the state lock still orders writers.
        self.inner.sync.publish_tabs(state.tabs.clone());
        Ok(())
    }
}
