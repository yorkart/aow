use anyhow::{Context, Result};
use aow_server_client::ServerClient;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
static REMOTE: OnceLock<Option<ServerClient>> = OnceLock::new();
pub fn initialize(url: Option<String>, token_file: Option<PathBuf>) -> Result<()> {
    let url = url.or_else(|| std::env::var("AOW_SERVER_URL").ok());
    let token_file =
        token_file.or_else(|| std::env::var_os("AOW_SERVER_TOKEN_FILE").map(PathBuf::from));
    let token = match token_file {
        Some(path) => {
            use std::os::unix::fs::PermissionsExt;
            anyhow::ensure!(
                std::fs::metadata(&path)?.permissions().mode() & 0o077 == 0,
                "Server token file must be private (0600)"
            );
            Some(std::fs::read_to_string(path)?.trim().to_owned())
        }
        None => std::env::var("AOW_SERVER_TOKEN").ok(),
    };
    let client = match (url, token) {
        (Some(url), Some(token)) => Some(ServerClient::remote(&url, token)?),
        (None, None) => None,
        _ => anyhow::bail!("Remote access requires Server URL and token"),
    };
    REMOTE
        .set(client)
        .map_err(|_| anyhow::anyhow!("Server connection already initialized"))?;
    Ok(())
}
pub fn client(state: &Path) -> Result<ServerClient> {
    if let Some(Some(client)) = REMOTE.get() {
        return Ok(client.clone());
    }
    Ok(ServerClient::new(
        std::env::current_dir()
            .context("Current directory is unavailable")?
            .join(state)
            .join("runtime/cli/cli.sock"),
    ))
}
