use super::paths::runtime_path;
use crate::{HostClient, HostError};
use crate::{
    TerminalAgentList, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec, TerminaldHealth,
};
use http::{Method, StatusCode};
#[derive(Clone, Debug)]
pub struct TerminalClient {
    pub(super) host: HostClient,
}
impl TerminalClient {
    pub fn host(&self) -> &HostClient {
        &self.host
    }
    pub fn new(host: HostClient) -> Self {
        Self { host }
    }
    /// Test fixtures publish endpoint credentials in a private connection file.
    pub fn for_test_config(path: std::path::PathBuf) -> Self {
        Self::new(HostClient::from_connection_file(path))
    }

    pub async fn health(&self) -> Result<TerminaldHealth, HostError> {
        let (status, body) = self
            .host
            .request(Method::GET, "/v1/terminal/health", None::<&()>)
            .await?;
        HostClient::json(status, body)
    }

    pub async fn screen(
        &self,
        id: &str,
    ) -> Result<Option<aow_host_contract::terminal::TerminalScreen>, HostError> {
        self.host
            .get_json(&format!("{}/screen", runtime_path(id)))
            .await
    }

    pub async fn list(&self) -> Result<Vec<TerminalRuntime>, HostError> {
        let (status, body) = self
            .host
            .request(Method::GET, "/v1/terminal/runtimes", None::<&()>)
            .await?;
        let list: TerminalRuntimeList = HostClient::json(status, body)?;
        Ok(list.runtimes)
    }

    pub async fn agents(&self) -> Result<TerminalAgentList, HostError> {
        let (status, body) = self
            .host
            .request(Method::GET, "/v1/terminal/agents", None::<&()>)
            .await?;
        HostClient::json(status, body)
    }

    pub async fn get(&self, id: &str) -> Result<Option<TerminalRuntime>, HostError> {
        let path = runtime_path(id);
        let (status, body) = self.host.request(Method::GET, &path, None::<&()>).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        HostClient::json(status, body).map(Some)
    }

    pub async fn create(
        &self,
        id: &str,
        spec: &TerminalRuntimeSpec,
    ) -> Result<TerminalRuntime, HostError> {
        let path = runtime_path(id);
        let (status, body) = self.host.request(Method::PUT, &path, Some(spec)).await?;
        HostClient::json(status, body)
    }

    pub async fn delete(&self, id: &str) -> Result<(), HostError> {
        let path = runtime_path(id);
        let (status, body) = self
            .host
            .request(Method::DELETE, &path, None::<&()>)
            .await?;
        if status.is_success() {
            Ok(())
        } else {
            Err(HostClient::status_error(status, &body))
        }
    }
}
