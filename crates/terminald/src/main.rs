use anyhow::Result;
use aow_terminald::run_with_shutdown_and_vt_worker;
use tracing_subscriber::EnvFilter;

mod node_worker;
mod options;
mod shutdown;

fn main() -> Result<()> {
    #[cfg(target_os = "macos")]
    let unified = aow_macos_log::init_from_env(c"terminald", "aow_terminald=info");
    #[cfg(not(target_os = "macos"))]
    let unified = false;
    if !unified {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("aow_terminald=info")),
            )
            .init();
    }
    let result = run();
    #[cfg(target_os = "macos")]
    if let Err(error) = &result {
        aow_macos_log::report_error(&format!("AoW terminald failed: {error:#}"));
    }
    result
}

#[tokio::main]
async fn run() -> Result<()> {
    let Some(options) = options::parse()? else {
        options::print_help();
        return Ok(());
    };
    let (vt_config, embedded_worker) =
        node_worker::configure(&options.vt_node, options.vt_worker).await;

    tracing::info!(socket = %options.socket.display(), "terminald listening");
    let result =
        run_with_shutdown_and_vt_worker(options.socket, shutdown::wait_for_signal(), vt_config)
            .await;
    // Keep the extracted script alive until the Node worker has stopped.
    drop(embedded_worker);
    result?;
    Ok(())
}
