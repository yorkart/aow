use std::path::{Path, PathBuf};

use aow_protocol::{
    TerminalAgentList, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec, TerminaldHealth,
};
use bytes::Bytes;
use http::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{
    TerminaldClientError,
    paths::runtime_path,
    response::{expect_empty, expect_json},
    socket::default_socket_path,
    transport,
};

#[derive(Debug, Clone)]
pub struct TerminaldClient {
    pub(super) socket: PathBuf,
}

impl TerminaldClient {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    pub fn default_socket() -> Self {
        Self::new(default_socket_path())
    }

    pub fn default_socket_path() -> PathBuf {
        default_socket_path()
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub async fn health(&self) -> Result<TerminaldHealth, TerminaldClientError> {
        let (status, body) = self.request(Method::GET, "/v1/health", None::<&()>).await?;
        expect_json(status, body, &[StatusCode::OK])
    }

    pub async fn screen(
        &self,
        id: &str,
    ) -> Result<Option<aow_protocol::TerminalScreen>, TerminaldClientError> {
        self.get_json(&format!("{}/screen", runtime_path(id))).await
    }

    /// Local HTTP/JSON transport, also used with the AoW CLI socket.
    pub async fn get_json<R: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<R, TerminaldClientError> {
        let (status, body) = self.request(Method::GET, path, None::<&()>).await?;
        expect_json(status, body, &[StatusCode::OK])
    }

    pub async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, TerminaldClientError> {
        let (status, body) = self.request(Method::POST, path, Some(request)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn list(&self) -> Result<Vec<TerminalRuntime>, TerminaldClientError> {
        let (status, body) = self
            .request(Method::GET, "/v1/runtimes", None::<&()>)
            .await?;
        let list: TerminalRuntimeList = expect_json(status, body, &[StatusCode::OK])?;
        Ok(list.runtimes)
    }

    pub async fn agents(&self) -> Result<TerminalAgentList, TerminaldClientError> {
        let (status, body) = self.request(Method::GET, "/v1/agents", None::<&()>).await?;
        // Web and terminald can be upgraded independently.
        if status == StatusCode::NOT_FOUND {
            return Ok(TerminalAgentList::default());
        }
        expect_json(status, body, &[StatusCode::OK])
    }

    pub async fn get(&self, id: &str) -> Result<Option<TerminalRuntime>, TerminaldClientError> {
        let path = runtime_path(id);
        let (status, body) = self.request(Method::GET, &path, None::<&()>).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        expect_json(status, body, &[StatusCode::OK]).map(Some)
    }

    pub async fn create(
        &self,
        id: &str,
        spec: &TerminalRuntimeSpec,
    ) -> Result<TerminalRuntime, TerminaldClientError> {
        let path = runtime_path(id);
        let (status, body) = self.request(Method::PUT, &path, Some(spec)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn delete(&self, id: &str) -> Result<(), TerminaldClientError> {
        let path = runtime_path(id);
        let (status, body) = self.request(Method::DELETE, &path, None::<&()>).await?;
        expect_empty(status, body, &[StatusCode::NO_CONTENT])
    }

    async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<(StatusCode, Bytes), TerminaldClientError> {
        transport::request(&self.socket, method, path, body).await
    }
}

impl Default for TerminaldClient {
    fn default() -> Self {
        Self::default_socket()
    }
}
