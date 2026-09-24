//! Unix-domain socket listener and daemon lifecycle.

use super::*;
use crate::VtWorkerConfig;

/// Resolve the socket path used by both the daemon binary and its clients.
pub fn default_socket_path() -> PathBuf {
    if let Some(path) = nonempty_env("AOW_TERMINALD_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = nonempty_env("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir)
            .join("aow-terminald")
            .join("terminald.sock");
    }
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/aow-terminald-{uid}/terminald.sock"))
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

/// Run a fresh terminald instance until the server exits.
pub async fn run(socket_path: PathBuf) -> Result<(), TerminaldError> {
    run_with_shutdown(socket_path, std::future::pending()).await
}

/// Variant of [`run`] that enables deterministic graceful shutdown in tests
/// and embedding applications.
pub async fn run_with_shutdown<F>(socket_path: PathBuf, shutdown: F) -> Result<(), TerminaldError>
where
    F: Future<Output = ()> + Send + 'static,
{
    run_with_shutdown_and_vt_worker(socket_path, shutdown, None).await
}

/// Run terminald with an optional headless-xterm sidecar. A missing or
/// unstartable sidecar is an optimization failure, never a terminal outage.
pub async fn run_with_shutdown_and_vt_worker<F>(
    socket_path: PathBuf,
    shutdown: F,
    vt_config: Option<VtWorkerConfig>,
) -> Result<(), TerminaldError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (listener, _socket_guard) = bind_socket(&socket_path).await?;
    let vt_worker = match vt_config {
        Some(config) => match VtWorker::start(config).await {
            Ok(worker) => Some(worker),
            Err(error) => {
                tracing::warn!(%error, "failed to start VT worker; using raw terminal replay");
                None
            }
        },
        None => None,
    };
    let state = DaemonState::with_vt_worker(vt_worker.as_ref().map(VtWorker::client));
    let router = router_with_state(state.clone());
    let (connection_shutdown, _) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let service = TowerToHyperService::new(router.clone());
                        let mut shutdown = connection_shutdown.subscribe();
                        connections.spawn(async move {
                            let connection = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .with_upgrades();
                            tokio::pin!(connection);
                            tokio::select! {
                                result = &mut connection => {
                                    if let Err(error) = result {
                                        tracing::debug!(%error, "terminald HTTP/1.1 connection stopped");
                                    }
                                }
                                changed = shutdown.changed() => {
                                    if changed.is_ok() {
                                        connection.as_mut().graceful_shutdown();
                                        if let Err(error) = connection.await {
                                            tracing::debug!(%error, "terminald HTTP/1.1 connection shutdown failed");
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "terminald failed to accept a Unix socket connection");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    // Stop runtime creation before closing connections. Existing WebSockets
    // receive a deletion error, and every child is killed and reaped before
    // the daemon reports a clean shutdown.
    let runtime_result = state.shutdown_all().await;
    if let Some(worker) = vt_worker {
        worker.shutdown().await;
    }
    let _ = connection_shutdown.send(true);
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            tracing::debug!(%error, "terminald connection task failed");
        }
    }
    runtime_result
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.dev() == self.device && metadata.ino() == self.inode {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn bind_socket(path: &Path) -> Result<(UnixListener, SocketGuard), TerminaldError> {
    if !path.is_absolute() {
        return Err(TerminaldError::UnsafeSocket(format!(
            "socket path must be absolute: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        TerminaldError::UnsafeSocket(format!("socket path has no parent: {}", path.display()))
    })?;
    secure_socket_parent(parent)?;
    remove_stale_socket(path).await?;

    let listener = UnixListener::bind(path)?;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    let metadata = std::fs::symlink_metadata(path)?;
    Ok((
        listener,
        SocketGuard {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        },
    ))
}

fn secure_socket_parent(parent: &Path) -> Result<(), TerminaldError> {
    let existed = parent.exists();
    if !existed {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(parent)?;
    }
    let metadata = std::fs::symlink_metadata(parent)?;
    let uid = unsafe { libc::geteuid() };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TerminaldError::UnsafeSocket(format!(
            "socket parent is not a real directory: {}",
            parent.display()
        )));
    }
    if metadata.uid() != uid {
        return Err(TerminaldError::UnsafeSocket(format!(
            "socket parent is not owned by uid {uid}: {}",
            parent.display()
        )));
    }
    if existed && metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(TerminaldError::UnsafeSocket(format!(
            "socket parent permissions must be 0700: {}",
            parent.display()
        )));
    }
    Ok(())
}

async fn remove_stale_socket(path: &Path) -> Result<(), TerminaldError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let uid = unsafe { libc::geteuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(TerminaldError::UnsafeSocket(format!(
            "existing socket path is not a same-uid Unix socket: {}",
            path.display()
        )));
    }
    match tokio::net::UnixStream::connect(path).await {
        Ok(_) => Err(TerminaldError::SocketInUse(path.to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            let current = match std::fs::symlink_metadata(path) {
                Ok(current) => current,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            if current.file_type().is_socket()
                && current.uid() == uid
                && current.dev() == metadata.dev()
                && current.ino() == metadata.ino()
            {
                std::fs::remove_file(path)?;
                Ok(())
            } else {
                Err(TerminaldError::UnsafeSocket(format!(
                    "socket path changed while checking staleness: {}",
                    path.display()
                )))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TerminaldError::UnsafeSocket(format!(
            "cannot safely prove socket is stale at {}: {error}",
            path.display()
        ))),
    }
}
