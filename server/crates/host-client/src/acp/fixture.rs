use super::AcpClient;
use crate::{HostClient, HostEndpoint, testing::TestHost};
use anyhow::Result;
use std::{path::PathBuf, sync::Arc};

#[async_trait::async_trait]
pub trait AcpFixture: Send + Sync + 'static {
    async fn settings(&self) -> Result<String>;
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
}

#[derive(Default)]
struct FixtureNode {
    host: tokio::sync::OnceCell<TestHost>,
    synchronized_settings: tokio::sync::Mutex<Option<String>>,
}

pub(super) struct Fixture {
    host: Arc<dyn AcpFixture>,
    root: PathBuf,
    _temporary: Option<Arc<tempfile::TempDir>>,
    node: Arc<FixtureNode>,
}
impl Fixture {
    pub async fn host(&self) -> Result<HostClient> {
        // Clients sharing a node must synchronize host snapshots in order. A
        // host publishes remotely before committing its local settings, so an
        // unchanged local snapshot must not restore the old remote settings.
        let mut synchronized = self.node.synchronized_settings.lock().await;
        let content = self.host.settings().await?;
        let node = self.node.host.get_or_try_init(|| async {
            tokio::fs::create_dir_all(&self.root).await?;
            tokio::fs::write(self.root.join("aow-acp.json"), &content).await?;
            let path = self.host.execution_path().await?.or_else(|| std::env::var_os("PATH"));
            if let Some(path) = path {
                tokio::fs::write(self.root.join("aow-settings.json"),serde_json::to_vec(&serde_json::json!({"execution_path":std::env::split_paths(&path).collect::<Vec<_>>()}))?).await?;
            }
            Ok::<_,anyhow::Error>(TestHost::with_state_dir(&self.root.join("host.json"),self.root.clone()).await)
        }).await?;
        let host = HostClient::new(node.endpoint.clone());
        if synchronized.as_ref() != Some(&content) {
            let previous: super::AcpSettings = host.get_json("/v1/acp/settings").await?;
            if previous.content != content {
                let _: super::AcpSettings = host
                    .put_json(
                        "/v1/acp/settings",
                        &serde_json::json!({"content":content,"revision":previous.revision}),
                    )
                    .await?;
            }
            *synchronized = Some(content);
        }
        Ok(host)
    }
}
impl AcpClient {
    pub fn for_test_host(host: Arc<dyn AcpFixture>, state: Option<PathBuf>) -> Result<Self> {
        let temporary = if state.is_none() {
            Some(Arc::new(tempfile::tempdir()?))
        } else {
            None
        };
        let root = state.unwrap_or_else(|| temporary.as_ref().unwrap().path().into());
        type Nodes = std::collections::HashMap<PathBuf, std::sync::Weak<FixtureNode>>;
        static NODES: std::sync::LazyLock<std::sync::Mutex<Nodes>> =
            std::sync::LazyLock::new(Default::default);
        let node = {
            let mut nodes = NODES.lock().unwrap();
            nodes.retain(|_, node| node.strong_count() > 0);
            let node = nodes
                .get(&root)
                .and_then(std::sync::Weak::upgrade)
                .unwrap_or_default();
            nodes.insert(root.clone(), Arc::downgrade(&node));
            node
        };
        let mut client = Self::new(HostClient::new(HostEndpoint::unavailable()));
        Arc::get_mut(&mut client.0).unwrap().fixture = Some(Fixture {
            host,
            root,
            _temporary: temporary,
            node,
        });
        Ok(client)
    }
    pub async fn test_host(&self) -> Result<HostClient> {
        self.remote().await
    }
    pub fn test_host_owner(&self) -> Arc<dyn Send + Sync> {
        let fixture = self.0.fixture.as_ref().expect("external fixture");
        Arc::new((fixture.node.clone(), fixture._temporary.clone()))
    }
    pub fn with_test_observer(mut self, observer: Arc<dyn super::AcpObserver>) -> Self {
        Arc::get_mut(&mut self.0).unwrap().observer = Some(observer);
        self
    }
    pub fn disconnect_all(&self) {
        let client = self.clone();
        tokio::spawn(async move {
            if let Ok(sessions) = client.sessions(None).await {
                for session in sessions {
                    if let Ok(Some(connection)) =
                        client.session_connection_info(&session.thread_id).await
                    {
                        let _ = client.disconnect(&connection.id).await;
                    }
                }
            }
        });
    }
    pub fn for_test_offline(&self) -> Self {
        Self::new(HostClient::new(HostEndpoint::unavailable()))
    }
}
