use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

/// Stable host boundary. Configuration ownership and filesystem policy belong to AoW.
#[async_trait]
pub trait AcpHost: Send + Sync + 'static {
    async fn turn_completed(&self, _completion: super::TurnCompletion) {}
    /// Invalidate host metadata views after creation, replacement, deletion or a title change.
    /// Called under the thread store lock; implementations must not block or reenter the service.
    fn session_metadata_changed(&self) {}
    /// Per-host environment overrides, shared by all sessions of an adapter.
    fn execution_environment(&self) -> std::collections::BTreeMap<String, String> {
        Default::default()
    }
    async fn settings(&self) -> Result<String>;
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>>;
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String>;
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()>;
}
