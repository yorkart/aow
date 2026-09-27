use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use anyhow::{Context, Result};
use aow_server::{AppState, BasePath, build_router, default_terminal_state_dir};
use aow_terminald_client::TerminaldClient;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    #[cfg(target_os = "macos")]
    let unified = aow_macos_log::init_from_env(c"server", "aow=info,tower_http=info");
    #[cfg(not(target_os = "macos"))]
    let unified = false;
    if !unified {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("aow=info,tower_http=info")),
            )
            .init();
    }
    let result = run();
    #[cfg(target_os = "macos")]
    if let Err(error) = &result {
        aow_macos_log::report_error(&format!("AoW server failed: {error:#}"));
    }
    result
}

#[tokio::main]
async fn run() -> Result<()> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 8282_u16;
    let mut frontend = PathBuf::from("frontend/dist");
    let mut base_path = std::env::var_os("AOW_BASE_PATH").unwrap_or_default();
    let mut state_dir = default_terminal_state_dir();
    let mut terminald_socket = TerminaldClient::default_socket_path();
    let mut initialize_only = false;
    let mut initialize_if_missing = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--base-path" => {
                base_path = arguments
                    .next()
                    .context("--base-path requires a value")?
                    .into()
            }
            "--initialize-state" => initialize_only = true,
            "--initialize-state-if-missing" => {
                initialize_only = true;
                initialize_if_missing = true;
            }
            "--host" => host = arguments.next().context("--host requires a value")?,
            "--port" => {
                port = arguments
                    .next()
                    .context("--port requires a value")?
                    .parse()
                    .context("invalid --port")?;
            }
            "--frontend" => {
                frontend = PathBuf::from(arguments.next().context("--frontend requires a value")?);
            }
            "--state-dir" => {
                state_dir =
                    PathBuf::from(arguments.next().context("--state-dir requires a value")?);
            }
            "--terminald-socket" => {
                terminald_socket = PathBuf::from(
                    arguments
                        .next()
                        .context("--terminald-socket requires a value")?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "Usage: aow-server [--host 127.0.0.1] [--port 8282] [--base-path PATH] [--frontend frontend/dist] [--state-dir PATH] [--terminald-socket PATH] [--initialize-state | --initialize-state-if-missing]\nHost: IPv4 or IPv6 address (for example 127.0.0.1 or ::1); default 127.0.0.1\nBase path: --base-path overrides AOW_BASE_PATH; default /"
                );
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    if initialize_only {
        if initialize_if_missing
            && let Some(config) = aow_config::ConfigRepository::open(&state_dir)?
            && config.directory().join("aow-projects.json").exists()
        {
            return Ok(());
        }
        aow_server::initialize_aow_state(&state_dir).await?;
        return Ok(());
    }
    let base_path = BasePath::parse(
        base_path
            .to_str()
            .context("base path must be valid UTF-8")?,
    )?;
    let address = listen_address(&host, port)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "AoW listening");
    println!("AoW: http://{local}{}/", base_path.as_str());
    let state = AppState::with_runtime_options(frontend, state_dir.clone(), terminald_socket)?
        .with_base_path(base_path);
    state.initialize_global_workspace().await?;
    if let Err(error) = state.reconcile_terminals().await {
        tracing::warn!(%error, "initial terminald reconciliation failed; terminal requests will retry");
    }
    state.start_agent_notifications();
    let cli = aow_server::start_local_cli(state.clone(), &state_dir).await?;
    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(anyhow::Error::from)?;
    cli.shutdown().await;
    Ok(())
}

fn listen_address(host: &str, port: u16) -> Result<SocketAddr> {
    host.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, port))
        // Preserve bracketed IPv6 hosts accepted by the original CLI.
        .or_else(|_| format!("{host}:{port}").parse())
        .context("invalid listen address")
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler can be installed");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::listen_address;

    #[test]
    fn listen_addresses_accept_ipv4_and_both_ipv6_host_notations() {
        for (host, expected) in [
            ("127.0.0.1", "127.0.0.1:8282"),
            ("0.0.0.0", "0.0.0.0:8282"),
            ("::1", "[::1]:8282"),
            ("[::1]", "[::1]:8282"),
            ("::", "[::]:8282"),
            ("[::]", "[::]:8282"),
            ("2001:db8::10", "[2001:db8::10]:8282"),
            ("[2001:db8::10]", "[2001:db8::10]:8282"),
        ] {
            assert_eq!(listen_address(host, 8282).unwrap().to_string(), expected);
        }
        assert_eq!(listen_address("::1", 0).unwrap().port(), 0);
    }

    #[test]
    fn listen_address_rejects_invalid_hosts_without_broadening_the_bind() {
        for host in [
            "",
            "localhost",
            "127.0.0.1:8282",
            "[::1",
            "::1]",
            "not-an-ip",
        ] {
            assert!(listen_address(host, 8282).is_err(), "{host}");
        }
    }
}
