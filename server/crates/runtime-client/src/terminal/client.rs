use super::paths::runtime_path;
use crate::{RuntimeClient, RuntimeError};
use crate::{
    TerminalAgentList, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec, TerminaldHealth,
};
use http::{Method, StatusCode};
#[derive(Clone, Debug)]
pub struct TerminalClient {
    pub(super) runtime: RuntimeClient,
}
impl TerminalClient {
    pub fn runtime(&self) -> &RuntimeClient {
        &self.runtime
    }
    pub fn new(runtime: RuntimeClient) -> Self {
        Self { runtime }
    }
    /// Test fixtures publish endpoint credentials in a private connection file.
    pub fn for_test_config(path: std::path::PathBuf) -> Self {
        Self::new(RuntimeClient::from_connection_file(path))
    }

    pub async fn health(&self) -> Result<TerminaldHealth, RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::GET, "/v1/terminal/health", None::<&()>)
            .await?;
        RuntimeClient::json(status, body)
    }

    pub async fn screen(
        &self,
        id: &str,
    ) -> Result<Option<aow_runtime_contract::terminal::TerminalScreen>, RuntimeError> {
        self.runtime
            .get_json(&format!("{}/screen", runtime_path(id)))
            .await
    }

    pub async fn list(&self) -> Result<Vec<TerminalRuntime>, RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::GET, "/v1/terminal/runtimes", None::<&()>)
            .await?;
        let list: TerminalRuntimeList = RuntimeClient::json(status, body)?;
        Ok(list.runtimes)
    }

    pub async fn agents(&self) -> Result<TerminalAgentList, RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::GET, "/v1/terminal/agents", None::<&()>)
            .await?;
        RuntimeClient::json(status, body)
    }

    pub async fn get(&self, id: &str) -> Result<Option<TerminalRuntime>, RuntimeError> {
        let path = runtime_path(id);
        let (status, body) = self
            .runtime
            .request(Method::GET, &path, None::<&()>)
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        RuntimeClient::json(status, body).map(Some)
    }

    pub async fn create(
        &self,
        id: &str,
        spec: &TerminalRuntimeSpec,
    ) -> Result<TerminalRuntime, RuntimeError> {
        let path = runtime_path(id);
        let (status, body) = self.runtime.request(Method::PUT, &path, Some(spec)).await?;
        RuntimeClient::json(status, body)
    }

    pub async fn delete(&self, id: &str) -> Result<(), RuntimeError> {
        let path = runtime_path(id);
        let (status, body) = self
            .runtime
            .request(Method::DELETE, &path, None::<&()>)
            .await?;
        if status.is_success() {
            Ok(())
        } else {
            Err(RuntimeClient::status_error(status, &body))
        }
    }
}
