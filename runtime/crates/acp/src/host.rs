use crate::manager::AdapterManager;
use acp_client::{
    AcpConnector, AcpHost, AdapterTransport, AgentInfo, ConnectionStatus, ExecutionEnvironment,
    TurnCompletion,
};
use anyhow::Result;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct NodeHost {
    pub host: Arc<dyn AcpHost>,
    pub adapters: AdapterManager,
}
#[async_trait::async_trait]
impl AcpHost for NodeHost {
    fn connector(&self) -> Option<Arc<dyn AcpConnector>> {
        Some(Arc::new(self.adapters.clone()))
    }
    fn session_metadata_changed(&self) {
        self.host.session_metadata_changed();
    }
    async fn turn_completed(&self, completion: TurnCompletion) {
        self.host.turn_completed(completion).await;
    }
    fn execution_environment(&self) -> Result<BTreeMap<String, String>> {
        self.host.execution_environment()
    }
    fn validate_execution_environment(&self, environment: &ExecutionEnvironment) -> Result<()> {
        environment.validate()
    }
    async fn resolve_cwd(&self, cwd: &Path) -> Result<PathBuf> {
        let path = tokio::fs::canonicalize(self.host.resolve_cwd(cwd).await?).await?;
        anyhow::ensure!(path.is_dir(), "ACP working directory must be a directory");
        Ok(path)
    }
    async fn settings(&self) -> Result<String> {
        self.host.settings().await
    }
    async fn execution_path(&self) -> Result<Option<OsString>> {
        self.host.execution_path().await
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        self.host.read_text_file(cwd, path).await
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        self.host.write_text_file(cwd, path, content).await
    }
    async fn agents(&self) -> Result<Vec<AgentInfo>> {
        self.adapters.agents().await
    }
    async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        self.adapters.registry(refresh).await
    }
    async fn install_agent(&self, id: &str) -> Result<()> {
        self.adapters.install_agent(id).await
    }
    async fn installation_status(&self, id: &str) -> Result<Option<ConnectionStatus>> {
        Ok(self.adapters.installation_status(id))
    }
}
#[async_trait::async_trait]
impl AcpConnector for AdapterManager {
    async fn connect(
        &self,
        agent_id: &str,
        cwd: &Path,
        execution: Option<&ExecutionEnvironment>,
        environment: &BTreeMap<String, String>,
        connection_id: &str,
    ) -> Result<AdapterTransport> {
        let mut command = self.prepare_adapter(agent_id).await?;
        command.env.extend(environment.clone());
        if let Some(execution) = execution {
            execution.validate()?;
            command.env.extend(execution.env.clone());
            command
                .env
                .insert("HOME".into(), execution.home.to_string_lossy().into_owned());
        }
        crate::transport::spawn(cwd, command, connection_id)
    }
}
