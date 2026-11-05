use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct HostEndpoint {
    pub(crate) url: reqwest::Url,
    pub(crate) token: String,
}
impl std::fmt::Debug for HostEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostEndpoint")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}
impl HostEndpoint {
    pub fn url(&self) -> &reqwest::Url {
        &self.url
    }

    pub fn new(url: &str, token: String) -> Result<Self> {
        let url = reqwest::Url::parse(url)?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "Host URL must be an HTTP(S) URL without credentials, query or fragment"
        );
        ensure!(
            !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_graphic()),
            "Invalid Host bearer token"
        );
        Ok(Self { url, token })
    }
    pub fn from_file(path: &Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct File {
            url: String,
            token: String,
        }
        let file: File = serde_json::from_slice(&std::fs::read(path)?)?;
        Self::new(&file.url, file.token)
    }
    pub fn from_env(state_dir: &Path) -> Result<Self> {
        let url = std::env::var("AOW_HOST_URL").unwrap_or_else(|_| "http://127.0.0.1:9760".into());
        let file = std::env::var_os("AOW_HOST_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("runtime/host-daemon/token"));
        let token = std::fs::read_to_string(&file)
            .with_context(|| format!("Read Host token {}", file.display()))?;
        Self::new(&url, token.trim().into())
    }
    /// An unreachable endpoint for metadata-only tests; it never starts a local Host daemon.
    pub fn unavailable() -> Self {
        Self::new(
            "http://127.0.0.1:1",
            "host-unavailable-test-token-0123456789abcdef".into(),
        )
        .unwrap()
    }
}
