use crate::{AcpHost, AdapterCommand, host::NodeHost, manager::AdapterManager};
use anyhow::Result;
use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};

/// The execution node composes the shared ACP client with its local adapter manager.
#[derive(Clone)]
pub struct AcpService {
    client: acp_client::AcpService,
    adapters: AdapterManager,
}
impl AcpService {
    pub fn new(host: Arc<dyn AcpHost>, state: Option<PathBuf>) -> Result<Self> {
        Self::with_storage(host, state.clone(), state)
    }
    pub fn with_storage(
        host: Arc<dyn AcpHost>,
        state: Option<PathBuf>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        let adapters = AdapterManager::new(host.clone(), cache.clone());
        let client = acp_client::AcpService::new(
            Arc::new(NodeHost {
                host,
                adapters: adapters.clone(),
            }),
            state,
        )?;
        let _ = adapters.inner.events.set(client.sync_events());
        Ok(Self { client, adapters })
    }
    pub async fn prepare_adapter(&self, id: &str) -> Result<AdapterCommand> {
        self.adapters.prepare_adapter(id).await
    }
    pub fn installation_status(&self, id: &str) -> Option<crate::ConnectionStatus> {
        self.adapters.installation_status(id)
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.adapters
            .inner
            .shutting_down
            .store(true, Ordering::Release);
        // An in-flight client connect may own the client lock while waiting for
        // adapter preparation. Drain preparation, then release its lock before
        // waiting for the client lock, so shutdown cannot invert that order.
        let preparing = self.adapters.inner.connect_lock.lock().await;
        drop(preparing);
        self.client.shutdown().await
    }
}
impl Deref for AcpService {
    type Target = acp_client::AcpService;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

#[cfg(test)]
mod tests;
