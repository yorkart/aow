//! Test adapters still execute behind the node's HTTP/WebSocket boundary.
use super::{AcpSettings, connector::Connector, host::RemoteHost};
use crate::{RuntimeClient, testing::TestRuntime};
use anyhow::Result;
use aow_acp_client::{
    AcpConnector, AcpHost, AdapterTransport, AgentInfo, ConnectionStatus, ExecutionEnvironment,
    TurnCompletion,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, OnceCell};

struct Node {
    fixture: TestRuntime,
    _root: tempfile::TempDir,
}
struct State {
    host: Arc<dyn AcpHost>,
    node: OnceCell<Node>,
    configuration: Mutex<()>,
}
pub(super) struct FixtureHost(Arc<State>);
impl FixtureHost {
    pub fn new(host: Arc<dyn AcpHost>) -> Self {
        Self(Arc::new(State {
            host,
            node: OnceCell::new(),
            configuration: Mutex::new(()),
        }))
    }
}
impl State {
    async fn runtime(&self) -> Result<RuntimeClient> {
        let node = self
            .node
            .get_or_try_init(async || {
                let root = tempfile::tempdir()?;
                let fixture = TestRuntime::with_state_dir(
                    &root.path().join("connection.json"),
                    root.path().into(),
                )
                .await;
                Ok::<_, anyhow::Error>(Node {
                    fixture,
                    _root: root,
                })
            })
            .await?;
        let runtime = RuntimeClient::new(node.fixture.endpoint.clone());
        let _guard = self.configuration.lock().await;
        let content = self.host.settings().await?;
        let previous: AcpSettings = runtime.get_json("/v1/acp/settings").await?;
        if previous.content != content {
            let _: AcpSettings = runtime
                .put_json(
                    "/v1/acp/settings",
                    &aow_runtime_contract::AcpSettingsUpdate {
                        content,
                        revision: previous.revision,
                    },
                )
                .await?;
        }
        if let Some(path) = self.host.execution_path().await? {
            let paths = std::env::split_paths(&path).collect::<Vec<_>>();
            let _: serde_json::Value = runtime
                .put_json(
                    "/v1/environment",
                    &serde_json::json!({"execution_path":paths}),
                )
                .await?;
        }
        Ok(runtime)
    }
}
#[async_trait::async_trait]
impl AcpHost for FixtureHost {
    fn connector(&self) -> Option<Arc<dyn AcpConnector>> {
        Some(Arc::new(FixtureConnector(self.0.clone())))
    }
    fn session_metadata_changed(&self) {
        self.0.host.session_metadata_changed();
    }
    async fn turn_completed(&self, completion: TurnCompletion) {
        self.0.host.turn_completed(completion).await;
    }
    fn execution_environment(&self) -> Result<BTreeMap<String, String>> {
        self.0.host.execution_environment()
    }
    async fn resolve_cwd(&self, cwd: &Path) -> Result<PathBuf> {
        RemoteHost {
            runtime: self.0.runtime().await?,
            host: self.0.host.clone(),
        }
        .resolve_cwd(cwd)
        .await
    }
    async fn settings(&self) -> Result<String> {
        self.0.host.settings().await
    }
    async fn execution_path(&self) -> Result<Option<OsString>> {
        self.0.host.execution_path().await
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        self.0.host.read_text_file(cwd, path).await
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        self.0.host.write_text_file(cwd, path, content).await
    }
    async fn agents(&self) -> Result<Vec<AgentInfo>> {
        Ok(self.0.runtime().await?.acp_agents().await?)
    }
    async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        let value: serde_json::Value = self
            .0
            .runtime()
            .await?
            .get_json(&format!("/v1/acp/registry?refresh={refresh}"))
            .await?;
        Ok(serde_json::from_value(value["agents"].clone())?)
    }
    async fn install_agent(&self, id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .0
            .runtime()
            .await?
            .post_json(
                &format!("/v1/acp/agents/{id}/install"),
                &serde_json::json!({}),
            )
            .await?;
        Ok(())
    }
    async fn installation_status(&self, id: &str) -> Result<Option<ConnectionStatus>> {
        Ok(self
            .0
            .runtime()
            .await?
            .get_json(&format!("/v1/acp/agents/{id}/installation"))
            .await?)
    }
}
struct FixtureConnector(Arc<State>);
#[async_trait::async_trait]
impl AcpConnector for FixtureConnector {
    async fn connect(
        &self,
        agent_id: &str,
        cwd: &Path,
        execution: Option<&ExecutionEnvironment>,
        environment: &BTreeMap<String, String>,
        connection_id: &str,
    ) -> Result<AdapterTransport> {
        Connector(self.0.runtime().await?)
            .connect(agent_id, cwd, execution, environment, connection_id)
            .await
    }
}
