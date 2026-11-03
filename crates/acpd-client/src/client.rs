use super::{
    Event, Health, RpcError,
    protocol::{Call, PROTOCOL_VERSION, Request},
};
use anyhow::{Context, Result, bail};
use aow_acp::{AcpHost, AcpService, SyncEvent};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{net::UnixStream, sync::broadcast};
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

#[derive(Clone)]
pub(super) enum Backend {
    Remote(PathBuf),
    Embedded(AcpService),
}

#[derive(Clone)]
pub struct AcpClient {
    pub(super) backend: Backend,
}

pub fn default_socket_path(state_dir: &Path) -> PathBuf {
    std::env::var_os("AOW_ACPD_SOCKET")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("runtime/acpd/acpd.sock"))
}

/// An unavailable execution service must not turn a durable message into a permanent failure.
pub fn is_unavailable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<aow_terminald_client::TerminaldClientError>()
            || cause.downcast_ref::<RpcError>().is_some_and(|error| {
                matches!(error.code.as_str(), "unavailable" | "unsupported_protocol")
            })
    })
}

pub fn is_auth_required(error: &anyhow::Error) -> bool {
    aow_acp::is_auth_required(error)
        || error.chain().any(|error| {
            error
                .downcast_ref::<RpcError>()
                .is_some_and(|error| error.code == "auth_required")
        })
}

impl From<AcpService> for AcpClient {
    fn from(service: AcpService) -> Self {
        Self {
            backend: Backend::Embedded(service),
        }
    }
}

impl AcpClient {
    /// No connection or fallback is attempted until an operation is requested.
    pub fn remote(socket: PathBuf) -> Self {
        Self {
            backend: Backend::Remote(socket),
        }
    }
    pub fn embedded_service(&self) -> Option<&AcpService> {
        match &self.backend {
            Backend::Embedded(service) => Some(service),
            _ => None,
        }
    }
    pub async fn probe(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        execution: aow_acp::ExecutionEnvironment,
    ) -> Result<()> {
        self.call(Request::Probe {
            agent_id: agent_id.into(),
            cwd,
            execution,
        })
        .await
    }
    pub fn is_remote(&self) -> bool {
        matches!(self.backend, Backend::Remote(_))
    }
    /// Explicit in-process hosting, used by embedding applications and tests.
    pub fn new(host: Arc<dyn AcpHost>, directory: Option<PathBuf>) -> Result<Self> {
        Ok(AcpService::new(host, directory)?.into())
    }
    pub fn with_storage(
        host: Arc<dyn AcpHost>,
        state: Option<PathBuf>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        Ok(AcpService::with_storage(host, state, cache)?.into())
    }
    pub async fn health(&self) -> Result<Health> {
        let Backend::Remote(socket) = &self.backend else {
            bail!("Embedded ACP has no daemon health endpoint");
        };
        let health: Health = aow_terminald_client::TerminaldClient::new(socket.clone())
            .get_json("/v1/health")
            .await?;
        anyhow::ensure!(
            health.protocol == PROTOCOL_VERSION,
            "Unsupported acpd protocol {}",
            health.protocol
        );
        Ok(health)
    }
    pub(super) async fn call<T: DeserializeOwned>(&self, request: Request) -> Result<T> {
        let Backend::Remote(socket) = &self.backend else {
            unreachable!("embedded methods dispatch locally");
        };
        // The envelope preserves stable ACP error categories across the transport.
        let reply: Result<serde_json::Value, RpcError> =
            aow_terminald_client::TerminaldClient::new(socket.clone())
                .post_json(
                    "/v1/rpc",
                    &Call {
                        protocol: PROTOCOL_VERSION,
                        language: aow_i18n::current_language(),
                        request,
                    },
                )
                .await
                .context("ACP daemon unavailable")?;
        Ok(serde_json::from_value(reply?)?)
    }
    pub async fn flush_threads_on_quit(&self) -> Result<()> {
        // Disconnecting a server never flushes or shuts down the daemon it uses.
        if let Backend::Embedded(service) = &self.backend {
            service.flush_threads_on_quit()?;
        }
        Ok(())
    }
    pub async fn subscribe_events(&self) -> Result<broadcast::Receiver<Event>> {
        let Backend::Remote(path) = &self.backend else {
            bail!("Completion delivery is owned by the embedded host");
        };
        let stream = UnixStream::connect(path).await?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(64 * 1024 * 1024))
            .max_frame_size(Some(64 * 1024 * 1024));
        let (mut socket, _) = tokio_tungstenite::client_async_with_config(
            "ws://aow-acpd/v1/events",
            stream,
            Some(config),
        )
        .await?;
        // The server sends ready only after installing its subscriptions.
        let ready = socket.next().await.context("acpd event stream closed")??;
        anyhow::ensure!(
            ready == Message::Text("ready".into()),
            "Unsupported acpd event stream"
        );
        let (sender, receiver) = broadcast::channel(512);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sender.closed() => break,
                    message = socket.next() => match message {
                        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                            Ok(event) => { if sender.send(event).is_err() { break; } },
                            Err(_) => break,
                        },
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                        _ => break,
                    }
                }
            }
        });
        Ok(receiver)
    }
    pub async fn subscribe_sync(&self) -> Result<broadcast::Receiver<SyncEvent>> {
        if let Backend::Embedded(service) = &self.backend {
            return Ok(service.subscribe_sync());
        }
        let mut events = self.subscribe_events().await?;
        let (sender, receiver) = broadcast::channel(512);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sender.closed() => break,
                    event = events.recv() => match event {
                        Ok(Event::Sync(event)) => { if sender.send(event).is_err() { break; } },
                        Ok(Event::Completed { .. }) => {},
                        // Close subscribers on loss: they must revalidate, never skip commits.
                        Err(_) => break,
                    }
                }
            }
        });
        Ok(receiver)
    }
    pub async fn acknowledge_completion(&self, id: &str) -> Result<()> {
        let Backend::Remote(socket) = &self.backend else {
            return Ok(());
        };
        let _: serde_json::Value = aow_terminald_client::TerminaldClient::new(socket.clone())
            .post_json("/v1/completions/ack", &serde_json::json!({"id":id}))
            .await?;
        Ok(())
    }
}
