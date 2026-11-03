use crate::{
    auth::{Auth, authorize},
    config::node_id,
};
use anyhow::{Context, Result, ensure};
use aow_acpd::AcpRuntime;
use aow_terminald::{TerminalRuntimeHost, configure_vt_worker};
use axum::{Json, Router, middleware, routing::get};
use std::{
    future::{Future, IntoFuture},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::{net::TcpListener, sync::watch};

/// No independent acpd/terminald process or local socket is started by this service.
pub struct Options {
    pub state_dir: PathBuf,
    pub listen: SocketAddr,
    pub token: String,
    pub allowed_origins: Vec<String>,
    pub vt_node: Option<PathBuf>,
    pub vt_worker: Option<PathBuf>,
}

pub async fn run_with_shutdown(
    options: Options,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    let listener = TcpListener::bind(options.listen).await?;
    run_with_listener(options, listener, shutdown).await
}

/// Supply a prebound listener, including an ephemeral port for tests and embedding.
pub async fn run_with_listener(
    options: Options,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    run_with_listener_and_vt_config(options, listener, shutdown, None).await
}

/// Embedding override for the PTY parser process; CLI deployments use node configuration.
pub async fn run_with_listener_and_vt_config(
    options: Options,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send,
    vt_override: Option<aow_terminald::VtWorkerConfig>,
) -> Result<()> {
    ensure!(
        options.state_dir.is_absolute(),
        "Node state directory must be absolute"
    );
    ensure!(
        options.token.len() >= 32
            && options.token.len() <= 4096
            && options.token.bytes().all(|b| b.is_ascii_graphic()),
        "Node bearer token must contain 32–4096 non-whitespace ASCII characters"
    );
    // One execution node owns each runtime state directory.
    let acp = AcpRuntime::open(options.state_dir.clone()).await?;
    let id = node_id(&options.state_dir)?;
    let (vt_config, _embedded_worker) = match options.vt_node {
        Some(node) => configure_vt_worker(&node, options.vt_worker).await,
        None => (None, None),
    };
    let mut terminal = TerminalRuntimeHost::with_environment(
        vt_override.or(vt_config),
        std::collections::BTreeMap::from([(
            "AOW_STATE_DIR".into(),
            std::fs::canonicalize(&options.state_dir)?
                .to_string_lossy()
                .into_owned(),
        )]),
    )
    .await;
    let instance_id = uuid::Uuid::new_v4().to_string();
    let health = serde_json::json!({
        "service": "aow-runtime-daemon", "api_version": 1,
        "node_id": id, "instance_id": instance_id,
        "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
        "capabilities": ["acp-websocket", "terminal-websocket", "runtime-management"],
    });
    let router = Router::new()
        .route("/v1/health", get(move || async move { Json(health) }))
        .merge(acp.router())
        .nest("/v1/terminal", terminal.router())
        .layer(middleware::from_fn_with_state(
            Auth {
                token: Arc::from(options.token),
                origins: Arc::new(options.allowed_origins),
            },
            authorize,
        ));
    tracing::info!(listen = %listener.local_addr()?, state_dir = %options.state_dir.display(), "runtime daemon listening");
    let (stop, mut stopping) = watch::channel(false);
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = stopping.changed().await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::pin!(shutdown);
    let server_result = tokio::select! {
        result = &mut server => Some(result),
        _ = &mut shutdown => None,
    };
    stop.send_replace(true);
    // Shut both admission gates and reap both kinds of child before reporting exit.
    let (acp_result, terminal_result) = tokio::join!(acp.shutdown(), terminal.shutdown());
    let server_result = match server_result {
        Some(result) => result,
        None => tokio::time::timeout(std::time::Duration::from_secs(10), &mut server)
            .await
            .context("Timed out draining node HTTP connections")?,
    };
    acp_result?;
    terminal_result?;
    server_result?;
    Ok(())
}
