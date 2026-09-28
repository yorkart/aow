use std::{ffi::OsString, path::PathBuf};

use anyhow::{Context, Result};
use aow_server::default_terminal_state_dir;
use aow_terminald_client::TerminaldClient;

pub(super) struct Options {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) frontend: PathBuf,
    pub(super) secure_cookies: bool,
    pub(super) base_path: OsString,
    pub(super) state_dir: PathBuf,
    pub(super) terminald_socket: PathBuf,
    pub(super) initialize_only: bool,
    pub(super) initialize_if_missing: bool,
}

pub(super) fn parse() -> Result<Option<Options>> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 8282_u16;
    let mut frontend = PathBuf::from("frontend/dist");
    let mut secure_cookies = match std::env::var("AOW_AUTH_SECURE_COOKIE") {
        Ok(value) if !value.is_empty() => value
            .parse::<bool>()
            .context("AOW_AUTH_SECURE_COOKIE must be true or false")?,
        Ok(_) | Err(std::env::VarError::NotPresent) => false,
        Err(error) => return Err(error).context("invalid AOW_AUTH_SECURE_COOKIE"),
    };
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
            "--secure-cookies" => secure_cookies = true,
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
                print_help();
                return Ok(None);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    Ok(Some(Options {
        host,
        port,
        frontend,
        secure_cookies,
        base_path,
        state_dir,
        terminald_socket,
        initialize_only,
        initialize_if_missing,
    }))
}

fn print_help() {
    println!(
        "Usage: aow-server [--host 127.0.0.1] [--port 8282] [--base-path PATH] [--secure-cookies] [--frontend frontend/dist] [--state-dir PATH] [--terminald-socket PATH] [--initialize-state | --initialize-state-if-missing]\nHost: IPv4 or IPv6 address (for example 127.0.0.1 or ::1); default 127.0.0.1\nBase path: --base-path overrides AOW_BASE_PATH; default /\nHTTPS: --secure-cookies or AOW_AUTH_SECURE_COOKIE=true marks session cookies Secure"
    );
}
