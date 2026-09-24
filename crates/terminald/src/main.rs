use std::{ffi::OsString, io::Write, path::PathBuf};

use anyhow::{Context, Result};
use aow_terminald::{VtWorkerConfig, default_socket_path, run_with_shutdown_and_vt_worker};
use tempfile::NamedTempFile;
use tracing_subscriber::EnvFilter;

const EMBEDDED_VT_WORKER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vt-worker.mjs"));
const MINIMUM_NODE_MAJOR: u64 = 20;

struct Options {
    socket: PathBuf,
    vt_node: PathBuf,
    vt_worker: Option<PathBuf>,
}

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
        aow_macos_log::report_error(&format!("AOW terminald failed: {error:#}"));
    }
    result
}

#[tokio::main]
async fn run() -> Result<()> {
    let Some(options) = parse_options()? else {
        print_help();
        return Ok(());
    };
    let mut embedded_worker = None;
    let vt_config = match validate_node(&options.vt_node).await {
        Ok(()) => match options.vt_worker {
            Some(worker) => worker_config_for_override(options.vt_node.clone(), worker),
            None => match extract_embedded_worker() {
                Ok(worker) => {
                    let config = VtWorkerConfig::new(options.vt_node.clone(), worker.path());
                    embedded_worker = Some(worker);
                    Some(config)
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to extract embedded VT worker; using raw terminal replay");
                    None
                }
            },
        },
        Err(error) => {
            tracing::warn!(
                node = %options.vt_node.display(),
                %error,
                "Node.js 20+ is unavailable; using raw terminal replay"
            );
            None
        }
    };

    tracing::info!(socket = %options.socket.display(), "terminald listening");
    let result =
        run_with_shutdown_and_vt_worker(options.socket, shutdown_signal(), vt_config).await;
    // Keep the extracted script alive until the Node worker has stopped.
    drop(embedded_worker);
    result?;
    Ok(())
}

fn parse_options() -> Result<Option<Options>> {
    let mut socket = default_socket_path();
    let mut vt_node =
        nonempty_env_path("AOW_TERMINALD_VT_NODE").unwrap_or_else(|| PathBuf::from("node"));
    let mut vt_worker = nonempty_env_path("AOW_TERMINALD_VT_WORKER");
    let mut arguments = std::env::args_os().skip(1);

    while let Some(argument) = arguments.next() {
        if argument == "--socket" {
            socket = PathBuf::from(next_value(&mut arguments, "--socket")?);
        } else if argument == "--vt-node" {
            vt_node = PathBuf::from(next_value(&mut arguments, "--vt-node")?);
        } else if argument == "--vt-worker" {
            vt_worker = Some(PathBuf::from(next_value(&mut arguments, "--vt-worker")?));
        } else if argument == "-h" || argument == "--help" {
            return Ok(None);
        } else {
            anyhow::bail!("unknown argument: {}", argument.to_string_lossy());
        }
    }

    Ok(Some(Options {
        socket,
        vt_node,
        vt_worker,
    }))
}

fn next_value(arguments: &mut impl Iterator<Item = OsString>, option: &str) -> Result<OsString> {
    let value = arguments
        .next()
        .with_context(|| format!("{option} requires a value"))?;
    if value.is_empty() {
        anyhow::bail!("{option} requires a non-empty value");
    }
    Ok(value)
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn worker_config_for_override(node: PathBuf, worker: PathBuf) -> Option<VtWorkerConfig> {
    match std::fs::metadata(&worker) {
        Ok(metadata) if metadata.is_file() => Some(VtWorkerConfig::new(node, worker)),
        Ok(_) => {
            tracing::warn!(
                worker = %worker.display(),
                "VT worker override is not a regular file; using raw terminal replay"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                worker = %worker.display(),
                %error,
                "VT worker override is unavailable; using raw terminal replay"
            );
            None
        }
    }
}

async fn validate_node(node: &PathBuf) -> Result<()> {
    let output = tokio::process::Command::new(node)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("run {} --version", node.display()))?;
    if !output.status.success() {
        anyhow::bail!("Node version command exited with {}", output.status);
    }
    let version = std::str::from_utf8(&output.stdout)
        .context("Node version output is not UTF-8")?
        .trim();
    let major = version
        .strip_prefix('v')
        .unwrap_or(version)
        .split('.')
        .next()
        .context("Node version output is empty")?
        .parse::<u64>()
        .with_context(|| format!("cannot parse Node version {version:?}"))?;
    if major < MINIMUM_NODE_MAJOR {
        anyhow::bail!("Node {version} is older than required v{MINIMUM_NODE_MAJOR}");
    }
    Ok(())
}

fn extract_embedded_worker() -> Result<NamedTempFile> {
    let mut worker = tempfile::Builder::new()
        .prefix("aow-terminald-vt-worker-")
        .suffix(".mjs")
        .tempfile()
        .context("create private temporary VT worker file")?;
    worker
        .write_all(EMBEDDED_VT_WORKER)
        .context("write embedded VT worker")?;
    worker.flush().context("flush embedded VT worker")?;
    Ok(worker)
}

fn print_help() {
    println!(
        "Usage: aow-terminald [--socket PATH] [--vt-node PATH] [--vt-worker PATH]\n\n\
         Environment:\n  \
         AOW_TERMINALD_SOCKET      Unix socket path\n  \
         AOW_TERMINALD_VT_NODE     Node.js 20+ executable\n  \
         AOW_TERMINALD_VT_WORKER   Development worker override; embedded bundle is the default"
    );
}

async fn shutdown_signal() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn node_version_validation_requires_node_20_or_newer() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old-node");
        std::fs::write(&old, "#!/bin/sh\nprintf 'v19.9.0\n'\n").unwrap();
        let current = directory.path().join("current-node");
        std::fs::write(&current, "#!/bin/sh\nprintf 'v20.0.0\n'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(validate_node(&old).await.is_err());
        validate_node(&current).await.unwrap();
    }
}
