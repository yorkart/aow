use anyhow::{Result, ensure};
use aow_runtime_daemon::{Config, Options, load_token};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Serve ACP and terminal runtimes from a single node daemon"
)]
struct Arguments {
    /// Existing AoW configuration/data root (defaults to AOW_STATE_DIR or ~/.aow).
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// Node transport configuration (defaults to STATE_DIR/runtime-daemon.json if present).
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    listen: Option<SocketAddr>,
    /// Private file containing a bearer token; default is created under the state directory.
    #[arg(long)]
    token_file: Option<PathBuf>,
    #[arg(long)]
    vt_node: Option<PathBuf>,
    #[arg(long)]
    vt_worker: Option<PathBuf>,
}

fn main() -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    #[cfg(target_os = "macos")]
    let unified = aow_macos_log::init_from_env(
        c"runtime-daemon",
        "aow_runtime_daemon=info,aow_acp_service=info,aow_terminal_core=info,aow_terminal_service=info",
    );
    #[cfg(not(target_os = "macos"))]
    let unified = false;
    if !unified {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    "aow_runtime_daemon=info,aow_acp_service=info,aow_terminal_core=info,aow_terminal_service=info".into()
                }),
            )
            .init();
    }
    let result = run();
    #[cfg(target_os = "macos")]
    if let Err(error) = &result {
        aow_macos_log::report_error(&format!("AoW runtime daemon failed: {error:#}"));
    }
    result
}

#[tokio::main]
async fn run() -> Result<()> {
    let args = Arguments::parse();
    let root = args
        .state_dir
        .unwrap_or_else(aow_filesystem::default_state_dir);
    ensure!(root.is_absolute(), "Node state directory must be absolute");
    let default_config = root.join("runtime-daemon.json");
    let mut config = match args.config {
        Some(path) => Config::load(&path)?,
        None if default_config.exists() => Config::load(&default_config)?,
        None => Config::default(),
    };
    if let Some(listen) = args.listen {
        config.listen = listen;
    }
    if let Some(path) = args.token_file {
        config.token_file = Some(path);
    }
    if let Some(path) = args.vt_node {
        config.vt_node = path;
    }
    if let Some(path) = args.vt_worker {
        config.vt_worker = Some(path);
    }
    let token = load_token(&root, config.token_file.as_deref())?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    aow_runtime_daemon::run_with_shutdown(
        Options {
            state_dir: root,
            listen: config.listen,
            token,
            allowed_origins: config.allowed_origins,
            vt_node: Some(config.vt_node),
            vt_worker: config.vt_worker,
        },
        async {
            tokio::select! { _ = interrupt.recv() => {}, _ = term.recv() => {} }
        },
    )
    .await
}
