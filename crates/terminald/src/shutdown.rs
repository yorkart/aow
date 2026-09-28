pub(super) async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(terminate) => terminate,
        Err(error) => {
            tracing::warn!(%error, "failed to install terminald SIGTERM handler");
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "failed to install terminald SIGINT handler");
            }
            return;
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                tracing::warn!(%error, "terminald SIGINT handler failed");
            }
        }
        _ = terminate.recv() => {}
    }
}
