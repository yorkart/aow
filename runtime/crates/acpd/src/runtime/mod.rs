//! Embeddable ACP adapter host. Network serving belongs to runtime-daemon.
mod probe;
use crate::host::Host;
use anyhow::{Result, ensure};
use aow_acp::AcpService;
use axum::Router;
pub use probe::AcpProber;
use std::{
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::watch;

pub struct AcpRuntime {
    service: AcpService,
    host: Arc<Host>,
    stopping: watch::Sender<bool>,
    wire: crate::wire::WireRuntime,
    conversations: crate::conversations::Conversations,
    lock: Arc<NodeOwnership>,
}
/// Shared lifetime of the node lock, including admitted blocking filesystem work.
pub struct NodeOwnership(std::fs::File);
impl Drop for NodeOwnership {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
impl AcpRuntime {
    pub async fn open(state_dir: PathBuf) -> Result<Self> {
        ensure!(
            state_dir.is_absolute(),
            "Runtime state directory must be absolute"
        );
        tokio::fs::create_dir_all(state_dir.join("data/runtime-daemon")).await?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(state_dir.join("data/runtime-daemon/daemon.lock"))?;
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Runtime state directory is already owned by another daemon"
        );
        let lock = Arc::new(NodeOwnership(lock));
        let host = Arc::new(Host {
            _ownership: lock.clone(),
            root: std::fs::canonicalize(&state_dir)?,
            stopping: std::sync::atomic::AtomicBool::new(false),
            filesystem: Default::default(),
            callback_environment: Default::default(),
            completions: crate::conversations::journal::CompletionLog::open(
                &state_dir.join("data/acp"),
            )?,
        });
        // The node is the sole owner of session identity, transcripts and adapter state.
        let service = AcpService::with_storage(
            host.clone(),
            Some(host.root.join("data/acp")),
            Some(host.root.join("cache/acp")),
        )?;
        let (stopping, stopped) = watch::channel(false);
        let conversations = crate::conversations::Conversations::new(
            service.clone(),
            host.clone(),
            stopped.clone(),
        );
        let wire = crate::wire::WireRuntime::new(service.clone(), host.clone(), stopped);
        Ok(Self {
            service,
            host,
            stopping,
            wire,
            conversations,
            lock,
        })
    }
    pub fn ownership(&self) -> Arc<NodeOwnership> {
        self.lock.clone()
    }
    pub fn prober(&self) -> AcpProber {
        AcpProber::new(self.service.clone(), self.stopping.subscribe())
    }
    pub fn router(&self) -> Router {
        self.wire.router().merge(self.conversations.router())
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.stopping.send_replace(true);
        self.host
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.wire.shutdown().await;
        self.service.shutdown().await?;
        let _files = self.host.filesystem.write().await;
        Ok(())
    }
}
impl Drop for AcpRuntime {
    fn drop(&mut self) {
        self.stopping.send_replace(true);
        self.host
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
#[cfg(test)]
mod tests;
