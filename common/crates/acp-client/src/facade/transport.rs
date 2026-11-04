//! Transport boundary between the ACP SDK/session model and an execution node.
use anyhow::Result;
use futures::{Sink, future::BoxFuture, stream::BoxStream};
use std::{io, path::Path, pin::Pin};
use tokio_util::sync::CancellationToken;

/// Single-message JSON-RPC streams and the actor which owns their transport.
/// Cancellation must close the transport and finish child/connection cleanup.
pub struct AdapterTransport {
    pub incoming: BoxStream<'static, io::Result<String>>,
    pub stderr: BoxStream<'static, io::Result<String>>,
    pub outgoing: Pin<Box<dyn Sink<String, Error = io::Error> + Send>>,
    pub stop: CancellationToken,
    pub lifetime: BoxFuture<'static, ()>,
}

#[async_trait::async_trait]
pub trait AcpConnector: Send + Sync + 'static {
    async fn connect(
        &self,
        agent_id: &str,
        cwd: &Path,
        execution: Option<&super::ExecutionEnvironment>,
        environment: &std::collections::BTreeMap<String, String>,
        connection_id: &str,
    ) -> Result<AdapterTransport>;
}
