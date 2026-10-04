//! Unix-domain socket listener and daemon lifecycle.

use super::*;
use crate::VtWorkerConfig;

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
    let (listener, _socket_guard) = super::socket::bind_socket(&socket_path).await?;
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
