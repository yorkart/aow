use super::paths::runtime_path;
use crate::{RuntimeClient, RuntimeError};
use crate::{
    TerminalAgentList, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec, TerminaldHealth,
};
use http::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
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
    ) -> Result<Option<aow_protocol::TerminalScreen>, RuntimeError> {
        self.get_json(&format!("{}/screen", runtime_path(id))).await
    }

    /// Authenticated runtime management request.
    pub async fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R, RuntimeError> {
        let (status, body) = self.runtime.request(Method::GET, path, None::<&()>).await?;
        RuntimeClient::json(status, body)
    }

    pub async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::POST, path, Some(request))
            .await?;
        RuntimeClient::json(status, body)
    }

    pub async fn put_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::PUT, path, Some(request))
            .await?;
        RuntimeClient::json(status, body)
    }

    pub async fn delete_json<T: Serialize + ?Sized>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<(), RuntimeError> {
        let (status, body) = self
            .runtime
            .request(Method::DELETE, path, Some(request))
            .await?;
        if status.is_success() {
            Ok(())
        } else {
            Err(RuntimeClient::status_error(status, &body))
        }
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
