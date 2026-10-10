use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

/// Stable host boundary. Configuration ownership and filesystem policy belong to AoW.
#[async_trait]
pub trait AcpHost: Send + Sync + 'static {
    async fn settings(&self) -> Result<String>;
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>>;
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String>;
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()>;
}
