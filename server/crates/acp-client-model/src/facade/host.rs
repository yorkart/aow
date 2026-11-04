use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

/// Stable host boundary. Configuration ownership and filesystem policy belong to AoW.
#[async_trait]
pub trait AcpHost: Send + Sync + 'static {
    /// Network clients provide a connector; execution hosts and protocol fixtures use stdio.
    fn connector(&self) -> Option<std::sync::Arc<dyn super::AcpConnector>> {
        None
    }

    async fn turn_completed(&self, _completion: super::TurnCompletion) {}
    /// Invalidate host metadata views after creation, replacement, deletion or a title change.
    /// Called under the thread store lock; implementations must not block or reenter the service.
    fn session_metadata_changed(&self) {}
    /// Per-host environment overrides, shared by all sessions of an adapter.
    fn execution_environment(&self) -> Result<std::collections::BTreeMap<String, String>> {
        Ok(Default::default())
    }
    async fn resolve_cwd(&self, cwd: &Path) -> Result<std::path::PathBuf> {
        let cwd = tokio::fs::canonicalize(cwd).await?;
        anyhow::ensure!(cwd.is_dir(), "ACP working directory must be a directory");
        Ok(cwd)
    }
    async fn settings(&self) -> Result<String>;
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>>;
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String>;
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()>;
}
