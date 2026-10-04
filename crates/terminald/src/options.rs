use std::{ffi::OsString, path::PathBuf};

use anyhow::{Context, Result};
use aow_terminald::default_socket_path;

pub(super) struct Options {
    pub(super) socket: PathBuf,
    pub(super) vt_node: PathBuf,
    pub(super) vt_worker: Option<PathBuf>,
}

pub(super) fn parse() -> Result<Option<Options>> {
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

pub(super) fn print_help() {
    println!(
        "Usage: aow-terminald [--socket PATH] [--vt-node PATH] [--vt-worker PATH]\n\n\
         Environment:\n  \
         AOW_TERMINALD_SOCKET      Unix socket path\n  \
         AOW_TERMINALD_VT_NODE     Node.js 20+ executable\n  \
         AOW_TERMINALD_VT_WORKER   Development worker override; embedded bundle is the default"
    );
}
