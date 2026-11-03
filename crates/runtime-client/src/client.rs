use super::{RuntimeEndpoint, RuntimeError, TerminalClient};
use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig},
};

pub type RuntimeSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
#[derive(Clone, Debug)]
pub struct RuntimeClient {
    endpoint: Arc<RuntimeEndpoint>,
    connection_file: Option<std::path::PathBuf>,
    http: reqwest::Client,
    acp_ready: Arc<tokio::sync::OnceCell<()>>,
}
impl RuntimeClient {
    pub fn new(endpoint: RuntimeEndpoint) -> Self {
        Self {
            endpoint: Arc::new(endpoint),
            connection_file: None,
            acp_ready: Arc::new(tokio::sync::OnceCell::new()),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(120))
                .build()
                .expect("runtime HTTP client"),
        }
    }
    pub fn from_connection_file(path: std::path::PathBuf) -> Self {
        let mut client = Self::new(RuntimeEndpoint::unavailable());
        client.connection_file = Some(path);
        client
    }
    fn resolved_endpoint(&self) -> Result<RuntimeEndpoint, RuntimeError> {
        match &self.connection_file {
            Some(path) => RuntimeEndpoint::from_file(path)
                .map_err(|error| RuntimeError::Io(std::io::Error::other(error.to_string()))),
            None => Ok((*self.endpoint).clone()),
        }
    }
    pub(crate) async fn acp_ready(&self) -> Result<(), RuntimeError> {
        self.acp_ready
            .get_or_try_init(|| async {
                let _: serde_json::Value = self
                    .post_json("/v1/workspaces/recover", &serde_json::json!({}))
                    .await?;
                Ok::<_, RuntimeError>(())
            })
            .await?;
        Ok(())
    }
    pub fn endpoint(&self) -> &RuntimeEndpoint {
        &self.endpoint
    }
    pub fn terminal(&self) -> TerminalClient {
        TerminalClient::new(self.clone())
    }
    pub async fn health(&self) -> Result<serde_json::Value, RuntimeError> {
        self.get_json("/v1/health").await
    }
    pub(crate) fn url(&self, endpoint: &RuntimeEndpoint, path: &str) -> reqwest::Url {
        let mut url = endpoint.url.clone();
        let base = url.path().trim_end_matches('/').to_owned();
        let (path, query) = path
            .split_once('?')
            .map_or((path, None), |(p, q)| (p, Some(q)));
        url.set_path(&format!(
            "{base}/{path}",
            path = path.trim_start_matches('/')
        ));
        url.set_query(query);
        url
    }
    pub(crate) async fn request<T: Serialize + ?Sized>(
        &self,
        method: http::Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<(http::StatusCode, Bytes), RuntimeError> {
        let endpoint = self.resolved_endpoint()?;
        let mut request = self
            .http
            .request(method, self.url(&endpoint, path))
            .bearer_auth(&endpoint.token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request.send().await?;
        let status = response.status();
        // Bound responses before buffering transcript/snapshot bodies.
        let mut bytes = bytes::BytesMut::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > 64 * 1024 * 1024 {
                return Err(RuntimeError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let bytes = bytes.freeze();
        Ok((status, bytes))
    }
    pub(crate) fn json<T: DeserializeOwned>(
        status: http::StatusCode,
        body: Bytes,
    ) -> Result<T, RuntimeError> {
        if !status.is_success() {
            return Err(Self::status_error(status, &body));
        }
        Ok(serde_json::from_slice(&body)?)
    }
    pub(crate) fn status_error(status: http::StatusCode, body: &[u8]) -> RuntimeError {
        let error = serde_json::from_slice::<aow_protocol::ApiError>(body).ok();
        let message = error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
        RuntimeError::HttpStatus {
            status,
            message,
            error,
        }
    }
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, RuntimeError> {
        let (status, body) = self.request(http::Method::GET, path, None::<&()>).await?;
        Self::json(status, body)
    }
    pub async fn put_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self.request(http::Method::PUT, path, Some(body)).await?;
        Self::json(status, body)
    }
    pub async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self.request(http::Method::POST, path, Some(body)).await?;
        Self::json(status, body)
    }
    pub async fn delete(&self, path: &str) -> Result<(), RuntimeError> {
        let (status, body) = self
            .request(http::Method::DELETE, path, None::<&()>)
            .await?;
        if !status.is_success() {
            return Err(Self::status_error(status, &body));
        }
        Ok(())
    }
    pub async fn websocket(&self, path: &str) -> Result<RuntimeSocket, RuntimeError> {
        let endpoint = self.resolved_endpoint()?;
        let mut url = self.url(&endpoint, path);
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).expect("WebSocket URL scheme");
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", endpoint.token)
                .parse()
                .expect("validated token"),
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(64 * 1024 * 1024))
            .max_frame_size(Some(64 * 1024 * 1024));
        let connection = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| {
            RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Runtime WebSocket handshake timed out",
            ))
        })??;
        Ok(connection.0)
    }
    pub async fn close_socket(mut socket: RuntimeSocket) {
        let _ = tokio::time::timeout(Duration::from_secs(5), socket.close(None)).await;
    }
}
