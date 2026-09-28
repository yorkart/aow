mod options;

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use aow_server::{AppState, BasePath, build_router};
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
    let Some(options) = options::parse()? else {
        return Ok(());
    };

    if options.initialize_only {
        if options.initialize_if_missing
            && let Some(config) = aow_config::ConfigRepository::open(&options.state_dir)?
            && config.directory().join("aow-projects.json").exists()
        {
            return Ok(());
        }
        aow_server::initialize_aow_state(&options.state_dir).await?;
        return Ok(());
    }
    let base_path = BasePath::parse(
        options
            .base_path
            .to_str()
            .context("base path must be valid UTF-8")?,
    )?;
    let address = listen_address(&options.host, options.port)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "AoW listening");
    println!("AoW: http://{local}{}/", base_path.as_str());
    let state = AppState::with_runtime_options(
        options.frontend,
        options.state_dir.clone(),
        options.terminald_socket,
    )?
    .with_base_path(base_path)
    .with_secure_cookies(options.secure_cookies);
    state.initialize_global_workspace().await?;
    if let Err(error) = state.reconcile_terminals().await {
        tracing::warn!(%error, "initial terminald reconciliation failed; terminal requests will retry");
    }
    state.start_agent_notifications();
    let cli = aow_server::start_local_cli(state.clone(), &options.state_dir).await?;
    axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
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
