use super::connector::Connector;
use crate::RuntimeClient;
use anyhow::Result;
use aow_acp::{AcpConnector, AcpHost, TurnCompletion};
use std::{collections::BTreeMap, ffi::OsString, path::Path, sync::Arc};

pub(super) struct RemoteHost {
    pub runtime: RuntimeClient,
    pub host: Arc<dyn AcpHost>,
}
#[async_trait::async_trait]
impl AcpHost for RemoteHost {
    fn connector(&self) -> Option<Arc<dyn AcpConnector>> {
        Some(Arc::new(Connector(self.runtime.clone())))
    }
    fn session_metadata_changed(&self) {
        self.host.session_metadata_changed()
    }
    async fn turn_completed(&self, value: TurnCompletion) {
        self.host.turn_completed(value).await
    }
    fn execution_environment(&self) -> BTreeMap<String, String> {
        self.host.execution_environment()
    }
    async fn resolve_cwd(&self, cwd: &Path) -> Result<std::path::PathBuf> {
        let value: serde_json::Value = self
            .runtime
            .post_json("/v1/directories/resolve", &serde_json::json!({"path":cwd}))
            .await?;
        Ok(std::path::PathBuf::from(
            value["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid directory response"))?,
        ))
    }
    async fn settings(&self) -> Result<String> {
        let value: super::client::AcpSettings = self.runtime.get_json("/v1/acp/settings").await?;
        Ok(value.content)
    }
    async fn execution_path(&self) -> Result<Option<OsString>> {
        #[derive(serde::Deserialize)]
        struct Environment {
            execution_path: Option<Vec<std::path::PathBuf>>,
        }
        let value: Environment = self.runtime.get_json("/v1/environment").await?;
        value
            .execution_path
            .map(std::env::join_paths)
            .transpose()
            .map_err(Into::into)
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        self.host.read_text_file(cwd, path).await
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        self.host.write_text_file(cwd, path, content).await
    }
}
