//! Embeddable ACP adapter host. Network serving belongs to runtime-daemon.
use crate::host::Host;
use anyhow::{Result, ensure};
use aow_acp::AcpService;
use axum::Router;
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
    lock: std::fs::File,
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
        let host = Arc::new(Host {
            root: std::fs::canonicalize(state_dir)?,
            stopping: std::sync::atomic::AtomicBool::new(false),
            filesystem: Default::default(),
        });
        // Session transcripts are owned by the ACP client, not the adapter execution node.
        let service =
            AcpService::with_storage(host.clone(), None, Some(host.root.join("cache/acp")))?;
        let (stopping, stopped) = watch::channel(false);
        let wire = crate::wire::WireRuntime::new(service.clone(), host.clone(), stopped);
        Ok(Self {
            service,
            host,
            stopping,
            wire,
            lock,
        })
    }
    pub fn router(&self) -> Router {
        self.wire.router()
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
        // Release ownership even while a fork temporarily retains the file description.
        unsafe {
            libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
#[cfg(test)]
mod tests;
