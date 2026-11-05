//! External-process fixture for caller tests; no caller links execution-node crates.
use anyhow::Result;
use aow_host_daemon::Options;
use serde::Deserialize;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

#[derive(Deserialize)]
struct Configuration {
    state_dir: PathBuf,
    connection_file: PathBuf,
    token: String,
    vt_node: Option<PathBuf>,
    vt_worker: Option<PathBuf>,
    vt_override: Option<aow_terminal_core::VtWorkerConfig>,
}
#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args_os().nth(1).expect("configuration file");
    let config: Configuration = serde_json::from_slice(&std::fs::read(path)?)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    std::fs::write(
        &config.connection_file,
        serde_json::json!({
            "url":format!("http://{address}"),"token":config.token,
        })
        .to_string(),
    )?;
    std::fs::set_permissions(
        &config.connection_file,
        std::fs::Permissions::from_mode(0o600),
    )?;
    aow_host_daemon::run_with_listener_and_vt_config(
        Options {
            state_dir: config.state_dir,
            listen: address,
            token: config.token,
            allowed_origins: Vec::new(),
            vt_node: config.vt_node,
            vt_worker: config.vt_worker,
        },
        listener,
        async {
            use tokio::io::AsyncReadExt;
            let _ = tokio::io::stdin().read_u8().await;
        },
        config.vt_override,
    )
    .await
}
