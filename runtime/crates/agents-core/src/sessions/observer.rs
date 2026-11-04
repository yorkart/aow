//! Read-only access to managed terminals, supplied by the embedding runtime.
use anyhow::Result;
use aow_protocol::{TerminalAgentList, TerminalRuntimeList};

#[async_trait::async_trait]
pub trait ManagedTerminals: Send + Sync {
    async fn agents(&self) -> Result<TerminalAgentList>;
    fn runtimes(&self) -> Result<TerminalRuntimeList>;
}
