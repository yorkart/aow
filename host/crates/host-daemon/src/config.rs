use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Node transport settings. Agent definitions remain in state_dir/aow-acp.json.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub token_file: Option<PathBuf>,
    pub allowed_origins: Vec<String>,
    pub vt_node: PathBuf,
    pub vt_worker: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9760),
            token_file: None,
            allowed_origins: Vec::new(),
            vt_node: "node".into(),
            vt_worker: None,
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut config: Self = serde_json::from_slice(
            &std::fs::read(path).with_context(|| format!("Read {}", path.display()))?,
        )
        .with_context(|| format!("Parse {}", path.display()))?;
        let directory = path
            .parent()
            .context("Configuration directory is missing")?;
        for entry in [&mut config.token_file, &mut config.vt_worker]
            .into_iter()
            .flatten()
        {
            if entry.is_relative() {
                *entry = directory.join(&*entry);
            }
        }
        Ok(config)
    }
}

/// Use a private existing token file, or atomically create the default node token.
/// Tokens are never printed; administrators can read the file to configure a server.
pub fn load_token(state_dir: &Path, explicit: Option<&Path>) -> Result<String> {
    let default = state_dir.join("runtime/host-daemon/token");
    let path = explicit.unwrap_or(&default);
    if explicit.is_none() {
        private_directory(default.parent().context("Token directory is missing")?)?;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(mut file) => {
                let token = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                file.write_all(token.as_bytes())?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("Open token file {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.permissions().mode() & 0o077 == 0,
        "Token file must be a regular file owned by this user with mode 0600 or stricter"
    );
    let mut token = String::new();
    file.take(4097).read_to_string(&mut token)?;
    let token = token.trim();
    ensure!(
        token.len() >= 32
            && token.len() <= 4096
            && token.bytes().all(|byte| byte.is_ascii_graphic()),
        "Bearer token must contain 32–4096 non-whitespace ASCII characters"
    );
    Ok(token.into())
}

pub(crate) fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
        "Node directory must be owned by this user and must not be a symlink"
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(crate) fn node_id(state_dir: &Path) -> Result<String> {
    let directory = state_dir.join("data/host-daemon");
    private_directory(&directory)?;
    let path = directory.join("node-id");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(uuid::Uuid::new_v4().to_string().as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let mut id = String::new();
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?
        .take(129)
        .read_to_string(&mut id)?;
    Ok(uuid::Uuid::parse_str(id.trim())
        .context("Invalid persisted node identity")?
        .to_string())
}
