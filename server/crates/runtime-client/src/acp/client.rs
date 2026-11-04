use super::host::RemoteHost;
use crate::RuntimeClient;
use anyhow::{Result, bail, ensure};
use aow_acp::{AcpHost, AcpService, ExecutionEnvironment, SessionCreation, SyncEvent};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AcpClient {
    pub(super) service: AcpService,
    pub(super) runtime: Option<RuntimeClient>,
}
#[derive(Serialize, Deserialize)]
pub struct AcpSettings {
    pub path: String,
    pub content: String,
    pub revision: String,
}
impl AcpClient {
    /// Production clients always use the authenticated runtime connector. Errors never fall back to stdio.
    pub fn with_runtime(
        runtime: RuntimeClient,
        host: Arc<dyn AcpHost>,
        state: Option<PathBuf>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        let service = AcpService::with_storage(
            Arc::new(RemoteHost {
                runtime: runtime.clone(),
                host,
            }),
            state,
            cache,
        )?;
        Ok(Self {
            service,
            runtime: Some(runtime),
        })
    }
    /// Explicit protocol/session fixture; not used by the server executable.
    pub fn for_test_host(host: Arc<dyn AcpHost>, state: Option<PathBuf>) -> Result<Self> {
        Self::for_test_storage(host, state.clone(), state)
    }
    pub fn for_test_storage(
        host: Arc<dyn AcpHost>,
        state: Option<PathBuf>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        Ok(Self {
            service: AcpService::with_storage(host, state, cache)?,
            runtime: None,
        })
    }
    pub fn test_service(&self) -> Option<&AcpService> {
        self.runtime.is_none().then_some(&self.service)
    }
    pub fn disconnect_all(&self) {
        for session in self.service.sessions(None) {
            if let Ok(Some(connection)) = self.service.session_connection_info(&session.thread_id) {
                let _ = self.service.disconnect(&connection.id);
            }
        }
    }
    pub async fn flush_threads_on_quit(&self) -> Result<()> {
        self.service.shutdown().await
    }
    pub async fn subscribe_sync(&self) -> Result<broadcast::Receiver<SyncEvent>> {
        if let Some(runtime) = &self.runtime {
            runtime.health().await?;
        }
        Ok(self.service.subscribe_sync())
    }
    pub async fn settings(&self) -> Result<AcpSettings> {
        self.runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Settings are owned by the explicit test host"))?
            .get_json("/v1/acp/settings")
            .await
            .map_err(Into::into)
    }
    pub async fn save_settings(&self, content: String, revision: String) -> Result<AcpSettings> {
        self.runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Settings are owned by the explicit test host"))?
            .put_json(
                "/v1/acp/settings",
                &serde_json::json!({"content":content,"revision":revision}),
            )
            .await
            .map_err(Into::into)
    }
    pub fn for_test_offline(&self) -> Self {
        Self {
            service: self.service.clone(),
            runtime: Some(RuntimeClient::new(crate::RuntimeEndpoint::unavailable())),
        }
    }
    pub fn runtime(&self) -> Option<&RuntimeClient> {
        self.runtime.as_ref()
    }
    pub async fn probe(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        execution: ExecutionEnvironment,
    ) -> Result<()> {
        let session = self
            .service
            .prepare_session_with_options(
                agent_id,
                cwd,
                SessionCreation {
                    execution_environment: Some(execution),
                    ..Default::default()
                },
            )
            .await?;
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            self.service.resume(&session.thread_id).await?;
            self.service.start_prompt(
                &session.thread_id,
                serde_json::from_value(serde_json::json!([{"type":"text","text":"ping"}]))?,
            )?;
            loop {
                let snapshot = self.service.snapshot(&session.thread_id)?;
                ensure!(
                    snapshot.permissions.is_empty(),
                    "Runtime requested permission during connection test"
                );
                if let Some(error) = snapshot.error {
                    bail!("{error}");
                }
                if snapshot.status == "idle" {
                    ensure!(
                        snapshot
                            .entries
                            .iter()
                            .any(|entry| entry.kind == "assistant"
                                && entry.content["text"]
                                    .as_str()
                                    .is_some_and(|text| !text.trim().is_empty())),
                        "Runtime did not return a response"
                    );
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        })
        .await;
        let _ = self.service.delete_session(&session.thread_id).await;
        result?
    }
}
impl From<AcpService> for AcpClient {
    fn from(service: AcpService) -> Self {
        Self {
            service,
            runtime: None,
        }
    }
}
