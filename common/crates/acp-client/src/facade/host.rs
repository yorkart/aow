use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

/// Stable host boundary. Configuration ownership and filesystem policy belong to AoW.
#[async_trait]
pub trait AcpHost: Send + Sync + 'static {
    /// The host supplies a transport; this library never launches an adapter.
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
    fn validate_execution_environment(
        &self,
        environment: &super::ExecutionEnvironment,
    ) -> Result<()> {
        environment.validate_shape()
    }
    async fn resolve_cwd(&self, cwd: &Path) -> Result<std::path::PathBuf> {
        anyhow::ensure!(cwd.is_absolute(), "ACP working directory must be absolute");
        Ok(cwd.to_owned())
    }
    async fn agents(&self) -> Result<Vec<super::AgentInfo>> {
        anyhow::bail!("ACP management is not provided by this host")
    }
    async fn registry(&self, _refresh: bool) -> Result<Vec<super::AgentInfo>> {
        anyhow::bail!("ACP management is not provided by this host")
    }
    async fn install_agent(&self, _id: &str) -> Result<()> {
        anyhow::bail!("ACP management is not provided by this host")
    }
    async fn installation_status(&self, _id: &str) -> Result<Option<super::ConnectionStatus>> {
        anyhow::bail!("ACP management is not provided by this host")
    }
    async fn settings(&self) -> Result<String>;
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>>;
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String>;
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()>;
}
