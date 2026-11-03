use crate::host::Host;
use anyhow::{Result, ensure};
use aow_acp::AcpService;
use aow_acpd_client::{
    Event, Health, RpcError,
    protocol::{Call, PROTOCOL_VERSION},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
    routing::{get, post},
};
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use std::{
    future::Future,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    sync::{broadcast, watch},
    task::JoinSet,
};

pub struct Options {
    pub state_dir: PathBuf,
    pub socket: PathBuf,
}
#[derive(Clone)]
struct Daemon {
    service: AcpService,
    host: Arc<Host>,
    instance_id: String,
    stopping: watch::Receiver<bool>,
}

mod local;

/// Owns the ACP store lock and both local compatibility and native ACP runtimes.
/// No listener is created, so an embedding daemon can serve the public router itself.
pub struct AcpRuntime {
    state: Daemon,
    stopping: watch::Sender<bool>,
    wire: crate::wire::WireRuntime,
    lock: std::fs::File,
}

impl Drop for AcpRuntime {
    fn drop(&mut self) {
        self.stopping.send_replace(true);
        self.state
            .host
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        // A concurrent fork can inherit this open file description until exec.
        // Explicitly unlock rather than depending on the final descriptor close,
        // so another daemon can acquire ownership immediately after shutdown.
        unsafe {
            libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl AcpRuntime {
    pub async fn open(state_dir: PathBuf) -> Result<Self> {
        ensure!(
            state_dir.is_absolute(),
            "acpd state directory must be absolute"
        );
        tokio::fs::create_dir_all(state_dir.join("data/acp")).await?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(state_dir.join("data/acp/daemon.lock"))?;
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "ACP state directory is already owned by another daemon"
        );
        let host = Arc::new(Host {
            root: std::fs::canonicalize(&state_dir)?,
            probe: false,
            filesystem: Default::default(),
            stopping: std::sync::atomic::AtomicBool::new(false),
            completed: broadcast::channel(512).0,
        });
        let service = AcpService::with_storage(
            host.clone(),
            Some(host.root.join("data/acp")),
            Some(host.root.join("cache/acp")),
        )?;
        let (stopping, stopped) = watch::channel(false);
        let wire = crate::wire::WireRuntime::new(service.clone(), stopped.clone());
        Ok(Self {
            state: Daemon {
                service,
                host,
                instance_id: uuid::Uuid::new_v4().to_string(),
                stopping: stopped,
            },
            stopping,
            wire,
            lock,
        })
    }

    /// Native ACP JSON-RPC over WebSocket and HTTP instance management.
    /// The private AoW facade RPC is deliberately excluded from this router.
    pub fn router(&self) -> Router {
        self.wire.router()
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.stopping.send_replace(true);
        self.state
            .host
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.wire.shutdown().await;
        self.state.service.shutdown().await?;
        let _files = self.state.host.filesystem.write().await;
        self.state.service.flush_threads_on_quit()
    }
}

/// Local compatibility process for existing AoW servers.
pub async fn run_with_shutdown(
    options: Options,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    let (listener, _socket) = crate::socket::bind_socket(&options.socket).await?;
    let runtime = AcpRuntime::open(options.state_dir).await?;
    let router = local::router(runtime.state.clone());
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Some(result) = connections.join_next(), if !connections.is_empty() => { if let Err(error) = result { tracing::debug!(%error, "acpd connection ended"); } },
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let router = router.clone();
                let mut stopped = runtime.stopping.subscribe();
                connections.spawn(async move {
                    let connection = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), TowerToHyperService::new(router)).with_upgrades();
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = stopped.changed() => { connection.as_mut().graceful_shutdown(); let _ = connection.await; }
                    }
                });
            }
        }
    }
    let result = runtime.shutdown().await;
    while connections.join_next().await.is_some() {}
    result
}

#[cfg(test)]
mod tests;
