use std::{ffi::OsString, path::PathBuf};

use anyhow::{Context, Result};
use aow_server::{SessionTimeouts, default_terminal_state_dir};

#[derive(Debug)]
pub(super) struct Options {
    pub server_url: Option<String>,
    pub cli_token_file: Option<PathBuf>,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) frontend: PathBuf,
    pub(super) secure_cookies: bool,
    pub(super) session_timeouts: SessionTimeouts,
    pub(super) base_path: OsString,
    pub(super) state_dir: PathBuf,
    pub(super) host_url: String,
    pub(super) host_token_file: PathBuf,
    pub(super) initialize_only: bool,
    pub(super) initialize_if_missing: bool,
}

pub(super) fn parse() -> Result<Option<Options>> {
    parse_from(std::env::args().skip(1), |key| std::env::var_os(key))
}

fn parse_from(
    mut arguments: impl Iterator<Item = String>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Result<Option<Options>> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 8282_u16;
    let mut frontend = PathBuf::from("frontend/dist");
    let mut secure_cookies = match environment("AOW_AUTH_SECURE_COOKIE") {
        Some(value) if !value.is_empty() => value
            .to_str()
            .context("invalid AOW_AUTH_SECURE_COOKIE")?
            .parse::<bool>()
            .context("AOW_AUTH_SECURE_COOKIE must be true or false")?,
        _ => false,
    };
    let mut session_max_age = environment("AOW_AUTH_SESSION_MAX_AGE_SECONDS");
    let mut session_idle_timeout = environment("AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS");
    let mut base_path = environment("AOW_BASE_PATH").unwrap_or_default();
    let mut state_dir = default_terminal_state_dir();
    let mut host_url = environment("AOW_HOST_URL")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "http://127.0.0.1:9760".into());
    let mut host_token_file = environment("AOW_HOST_TOKEN_FILE").map(PathBuf::from);
    let mut server_url =
        environment("AOW_SERVER_PUBLIC_URL").map(|v| v.to_string_lossy().into_owned());
    let mut cli_token_file = environment("AOW_SERVER_CLI_TOKEN_FILE").map(PathBuf::from);
    let mut initialize_only = false;
    let mut initialize_if_missing = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--server-url" => {
                server_url = Some(arguments.next().context("--server-url requires a value")?)
            }
            "--cli-token-file" => {
                cli_token_file = Some(
                    arguments
                        .next()
                        .context("--cli-token-file requires a value")?
                        .into(),
                )
            }
            "--base-path" => {
                base_path = arguments
                    .next()
                    .context("--base-path requires a value")?
                    .into()
            }
            "--secure-cookies" => secure_cookies = true,
            "--session-max-age-seconds" => {
                session_max_age = Some(
                    arguments
                        .next()
                        .context("--session-max-age-seconds requires a value")?
                        .into(),
                );
            }
            "--session-idle-timeout-seconds" => {
                session_idle_timeout = Some(
                    arguments
                        .next()
                        .context("--session-idle-timeout-seconds requires a value")?
                        .into(),
                );
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
            "--host-url" => host_url = arguments.next().context("--host-url requires a value")?,
            "--host-token-file" => {
                host_token_file = Some(PathBuf::from(
                    arguments
                        .next()
                        .context("--host-token-file requires a value")?,
                ))
            }
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    let session_timeouts = SessionTimeouts::from_seconds(
        timeout_seconds(
            session_max_age,
            "AOW_AUTH_SESSION_MAX_AGE_SECONDS / --session-max-age-seconds",
            SessionTimeouts::DEFAULT_MAX_AGE_SECONDS,
        )?,
        timeout_seconds(
            session_idle_timeout,
            "AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS / --session-idle-timeout-seconds",
            SessionTimeouts::DEFAULT_IDLE_TIMEOUT_SECONDS,
        )?,
    )?;

    Ok(Some(Options {
        server_url,
        cli_token_file,
        host,
        port,
        frontend,
        secure_cookies,
        session_timeouts,
        base_path,
        host_url,
        host_token_file: host_token_file
            .unwrap_or_else(|| state_dir.join("runtime/host-daemon/token")),
        state_dir,
        initialize_only,
        initialize_if_missing,
    }))
}

fn timeout_seconds(value: Option<OsString>, name: &str, default: u64) -> Result<u64> {
    let Some(value) = value else {
        return Ok(default);
    };
    let seconds = value
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .with_context(|| {
            format!(
                "{name} must be a positive integer in seconds (1..={})",
                u64::MAX
            )
        })?;
    Ok(seconds)
}

fn print_help() {
    println!(
        "Usage: aow-server [--host 127.0.0.1] [--port 8282] [--base-path PATH] [--secure-cookies] [--frontend frontend/dist] [--state-dir PATH] [--host-url URL] [--host-token-file PATH] [--server-url PUBLIC_URL] [--cli-token-file PATH] [--initialize-state | --initialize-state-if-missing]\nHost: IPv4 or IPv6 address (for example 127.0.0.1 or ::1); default 127.0.0.1\nBase path: --base-path overrides AOW_BASE_PATH; default /\nHTTPS: --secure-cookies or AOW_AUTH_SECURE_COOKIE=true marks session cookies Secure"
    );
    println!(
        "Session max age: --session-max-age-seconds overrides AOW_AUTH_SESSION_MAX_AGE_SECONDS; default 43200\nSession idle timeout: --session-idle-timeout-seconds overrides AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS; default 1800\nSession timeouts must be positive integers in seconds"
    );
}

#[cfg(test)]
mod tests;
