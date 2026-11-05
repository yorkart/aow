//! A terminal runtime owned by an embedding daemon, without a separate listener.

use super::*;
use crate::VtWorkerConfig;

pub struct TerminalRuntimeHost {
    state: DaemonState,
    worker: Option<VtWorker>,
}

impl TerminalRuntimeHost {
    pub async fn start(config: Option<VtWorkerConfig>) -> Self {
        Self::with_environment(config, Default::default()).await
    }

    /// Defaults are merged below per-instance environment and environment removals.
    pub async fn with_environment(
        config: Option<VtWorkerConfig>,
        environment: std::collections::BTreeMap<String, String>,
    ) -> Self {
        let worker = match config {
            Some(config) => match VtWorker::start(config).await {
                Ok(worker) => Some(worker),
                Err(error) => {
                    tracing::warn!(%error, "failed to start VT worker; using raw terminal replay");
                    None
                }
            },
            None => None,
        };
        Self {
            state: DaemonState::with_environment(
                worker.as_ref().map(VtWorker::client),
                environment,
            ),
            worker,
        }
    }

    pub fn manager(&self) -> TerminalManager {
        TerminalManager::new(self.state.clone())
    }

    pub async fn shutdown(&mut self) -> Result<(), TerminalError> {
        let result = self.state.shutdown_all().await;
        if let Some(worker) = self.worker.take() {
            worker.shutdown().await;
        }
        result
    }
}

/// Read-only node-local view used by native-session services.
#[derive(Clone)]
pub struct TerminalObserver {
    pub(super) state: DaemonState,
}
impl TerminalRuntimeHost {
    pub fn observer(&self) -> TerminalObserver {
        TerminalObserver {
            state: self.state.clone(),
        }
    }
}
impl TerminalObserver {
    pub async fn agents(&self) -> Result<TerminalAgentList, TerminalError> {
        let state = self.state.clone();
        let mut detected = aow_i18n::spawn_blocking(move || state.agents())
            .await
            .map_err(|error| TerminalError::Worker(error.to_string()))??;
        super::session_titles::enrich(&mut detected).await;
        Ok(detected)
    }
    pub fn runtimes(&self) -> Result<TerminalRuntimeList, TerminalError> {
        self.state.list()
    }
}

#[async_trait::async_trait]
impl aow_agent_runtime_core::sessions::observer::ManagedTerminals for TerminalObserver {
    async fn agents(&self) -> anyhow::Result<TerminalAgentList> {
        Ok(TerminalObserver::agents(self).await?)
    }

    fn runtimes(&self) -> anyhow::Result<TerminalRuntimeList> {
        Ok(TerminalObserver::runtimes(self)?)
    }
}
