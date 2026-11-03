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

    pub fn router(&self) -> Router {
        super::http::routes_with_state(self.state.clone())
    }

    pub async fn shutdown(&mut self) -> Result<(), TerminaldError> {
        let result = self.state.shutdown_all().await;
        if let Some(worker) = self.worker.take() {
            worker.shutdown().await;
        }
        result
    }
}
