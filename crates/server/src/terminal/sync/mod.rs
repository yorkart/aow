//! Shared, independently bounded samplers; browsers receive snapshots only on change.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use tokio::sync::{Notify, watch};
use tokio::time::{Duration, MissedTickBehavior};

use super::{TerminalAgentList, TerminalError, TerminalManager, TerminalTab};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(1500);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(3);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Sample {
    Tabs,
    Agents,
}

#[derive(Clone, Serialize)]
pub(crate) struct Snapshot {
    pub(crate) boot_id: String,
    pub(crate) revision: u64,
    pub(crate) tabs: Option<Vec<TerminalTab>>,
    pub(crate) metadata: Option<TerminalAgentList>,
    pub(crate) error: Option<String>,
    pub(crate) agent_error: Option<String>,
}

pub(super) struct TerminalSync {
    changes: watch::Sender<Snapshot>,
    started: AtomicBool,
    refresh_tabs: Arc<Notify>,
    refresh_agents: Arc<Notify>,
}

impl Default for TerminalSync {
    fn default() -> Self {
        Self {
            changes: watch::channel(Snapshot {
                boot_id: uuid::Uuid::new_v4().to_string(),
                revision: 0,
                tabs: None,
                metadata: None,
                error: None,
                agent_error: None,
            })
            .0,
            started: AtomicBool::new(false),
            refresh_tabs: Arc::new(Notify::new()),
            refresh_agents: Arc::new(Notify::new()),
        }
    }
}

impl TerminalSync {
    pub(super) fn publish_tabs(&self, tabs: Vec<TerminalTab>) {
        self.changes.send_if_modified(|snapshot| {
            if snapshot.tabs.as_ref() == Some(&tabs) {
                return false;
            }
            snapshot.tabs = Some(tabs);
            snapshot.revision += 1;
            true
        });
    }

    fn publish_error(&self, error: Option<String>) {
        self.changes.send_if_modified(|snapshot| {
            if snapshot.error == error {
                return false;
            }
            snapshot.error = error;
            snapshot.revision += 1;
            true
        });
    }

    fn publish_agents(&self, result: Result<TerminalAgentList, super::TerminalError>) {
        self.changes.send_if_modified(|snapshot| {
            let (metadata, error) = match result {
                Ok(metadata) => (Some(metadata), None),
                // A failed detection must not make running agents disappear.
                Err(error) => (snapshot.metadata.clone(), Some(error.to_string())),
            };
            if snapshot.metadata == metadata && snapshot.agent_error == error {
                return false;
            }
            snapshot.metadata = metadata;
            snapshot.agent_error = error;
            snapshot.revision += 1;
            true
        });
    }
}

impl TerminalManager {
    pub(crate) fn subscribe_snapshots(&self) -> watch::Receiver<Snapshot> {
        let receiver = self.inner.sync.changes.subscribe();
        if !self.inner.sync.started.swap(true, Ordering::AcqRel) {
            self.start_sampler(Sample::Tabs);
            self.start_sampler(Sample::Agents);
        }
        self.refresh_snapshots();
        receiver
    }

    fn start_sampler(&self, sample: Sample) {
        let weak = Arc::downgrade(&self.inner);
        let (refresh, interval) = match sample {
            Sample::Tabs => (self.inner.sync.refresh_tabs.clone(), RECONCILE_INTERVAL),
            Sample::Agents => (self.inner.sync.refresh_agents.clone(), SAMPLE_INTERVAL),
        };
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = refresh.notified() => {},
                    _ = timer.tick() => {},
                }
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                if inner.sync.changes.receiver_count() == 0 {
                    continue;
                }
                let manager = TerminalManager { inner };
                // Each worker has at most one in-flight operation. A stuck daemon
                // request or title lookup cannot block the other worker or retries.
                match sample {
                    Sample::Tabs => {
                        let result = tokio::time::timeout(SAMPLE_TIMEOUT, manager.reconcile())
                            .await
                            .unwrap_or_else(|_| {
                                Err(TerminalError::DaemonUnavailable(
                                    "terminal reconciliation timed out".into(),
                                ))
                            });
                        manager
                            .inner
                            .sync
                            .publish_error(result.as_ref().err().map(ToString::to_string));
                        // Read under the commit lock; an asynchronous sample must
                        // never overwrite a newer committed tab list.
                        if result.is_ok()
                            && let Ok(state) = manager.lock_state()
                        {
                            manager.inner.sync.publish_tabs(state.tabs.clone());
                        }
                    }
                    Sample::Agents => {
                        let result = tokio::time::timeout(SAMPLE_TIMEOUT, manager.agents(None))
                            .await
                            .unwrap_or_else(|_| {
                                Err(TerminalError::DaemonUnavailable(
                                    "terminal agent detection timed out".into(),
                                ))
                            });
                        manager.inner.sync.publish_agents(result);
                    }
                }
            }
        });
    }

    pub(crate) fn refresh_snapshots(&self) {
        self.inner.sync.refresh_tabs.notify_one();
        self.inner.sync.refresh_agents.notify_one();
    }
}

#[cfg(test)]
mod tests;
