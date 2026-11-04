use super::*;
use crate::RuntimeClient;
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, broadcast};

/// Business notifications and callback credentials only. This is not an execution host.
#[async_trait::async_trait]
pub trait AcpObserver: Send + Sync + 'static {
    fn session_metadata_changed(&self) {}
    async fn turn_completed(&self, _completion: TurnCompletion) {}
    fn completion_cursor(&self) -> Result<Option<ConversationCursor>> {
        Ok(None)
    }
    fn save_completion_cursor(&self, _cursor: &ConversationCursor) -> Result<()> {
        Ok(())
    }
    fn callback_environment(&self) -> Result<BTreeMap<String, String>> {
        Ok(Default::default())
    }
}
#[derive(Clone)]
pub struct AcpClient(pub(super) Arc<Inner>);
pub(super) struct Inner {
    pub runtime: RuntimeClient,
    pub observer: Option<Arc<dyn AcpObserver>>,
    pub events: broadcast::Sender<SyncEvent>,
    pub resources: std::sync::Mutex<std::collections::HashSet<SyncResource>>,
    pub event_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub native: Mutex<Option<native::Connection>>,
    #[cfg(feature = "test-support")]
    pub fixture: Option<fixture::Fixture>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(task) = self.event_task.get_mut().take() {
            task.abort();
        }
    }
}
#[derive(Debug)]
pub(super) struct RemoteOperationError(pub ServiceError);
impl std::fmt::Display for RemoteOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.message)
    }
}
impl std::error::Error for RemoteOperationError {}
impl AcpClient {
    pub fn new(runtime: RuntimeClient) -> Self {
        Self::with_observer(runtime, None)
    }
    pub fn with_observer(runtime: RuntimeClient, observer: Option<Arc<dyn AcpObserver>>) -> Self {
        Self(Arc::new(Inner {
            runtime,
            observer,
            resources: Default::default(),
            events: broadcast::channel(512).0,
            event_task: Mutex::new(None),
            native: Mutex::new(None),
            #[cfg(feature = "test-support")]
            fixture: None,
        }))
    }
    pub fn runtime(&self) -> Option<&RuntimeClient> {
        #[cfg(feature = "test-support")]
        if self.0.fixture.is_some() {
            return None;
        }
        Some(&self.0.runtime)
    }
    pub(super) async fn remote(&self) -> Result<RuntimeClient> {
        #[cfg(feature = "test-support")]
        if let Some(fixture) = &self.0.fixture {
            return fixture.runtime().await;
        }
        Ok(self.0.runtime.clone())
    }
    pub(super) async fn call<T: DeserializeOwned>(&self, operation: Operation) -> Result<T> {
        let runtime = self.remote().await?;
        self.ensure_events(&runtime).await?;
        let callback_environment = self
            .0
            .observer
            .as_ref()
            .map(|observer| observer.callback_environment())
            .transpose()?;
        let result: Result<serde_json::Value, ServiceError> = runtime
            .post_json(
                "/v1/acp/conversations",
                &ServiceRequest {
                    operation,
                    language: Some(aow_i18n::current_language().tag().into()),
                    callback_environment,
                },
            )
            .await?;
        Ok(serde_json::from_value(
            result.map_err(RemoteOperationError)?,
        )?)
    }
    pub async fn flush_threads_on_quit(&self) -> Result<()> {
        self.call(Operation::Checkpoint).await
    }
    pub async fn subscribe_sync(&self) -> Result<broadcast::Receiver<SyncEvent>> {
        let receiver = self.0.events.subscribe();
        self.ensure_events(&self.remote().await?).await?;
        Ok(receiver)
    }
    pub async fn settings(&self) -> Result<AcpSettings> {
        Ok(self.remote().await?.get_json("/v1/acp/settings").await?)
    }
    pub async fn save_settings(&self, content: String, revision: String) -> Result<AcpSettings> {
        Ok(self
            .remote()
            .await?
            .put_json(
                "/v1/acp/settings",
                &serde_json::json!({"content":content,"revision":revision}),
            )
            .await?)
    }
    pub async fn probe(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        execution: ExecutionEnvironment,
    ) -> Result<()> {
        self.call(Operation::Probe {
            agent_id: agent_id.into(),
            cwd,
            execution,
        })
        .await
    }
}
