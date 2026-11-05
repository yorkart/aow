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
}
impl RuntimeClient {
    pub fn new(endpoint: RuntimeEndpoint) -> Self {
        Self {
            endpoint: Arc::new(endpoint),
            connection_file: None,
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
    pub fn is_configured(&self) -> bool {
        self.connection_file
            .as_ref()
            .is_none_or(|path| path.is_file())
    }
    fn resolved_endpoint(&self) -> Result<RuntimeEndpoint, RuntimeError> {
        match &self.connection_file {
            Some(path) => RuntimeEndpoint::from_file(path)
                .map_err(|error| RuntimeError::Io(std::io::Error::other(error.to_string()))),
            None => Ok((*self.endpoint).clone()),
        }
    }
    pub fn endpoint(&self) -> &RuntimeEndpoint {
        &self.endpoint
    }
    pub fn terminal(&self) -> TerminalClient {
        TerminalClient::new(self.clone())
    }
    pub async fn health(&self) -> Result<aow_runtime_contract::node::RuntimeHealth, RuntimeError> {
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
            .bearer_auth(&endpoint.token)
            .header(
                http::header::ACCEPT_LANGUAGE,
                aow_i18n::current_language().tag(),
            );
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
        let error = serde_json::from_slice::<aow_runtime_contract::error::ApiError>(body).ok();
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
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, RuntimeError> {
        let (status, body) = self.request(http::Method::GET, path, None::<&()>).await?;
        Self::json(status, body)
    }
    pub(crate) async fn put_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self.request(http::Method::PUT, path, Some(body)).await?;
        Self::json(status, body)
    }
    pub(crate) async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, RuntimeError> {
        let (status, body) = self.request(http::Method::POST, path, Some(body)).await?;
        Self::json(status, body)
    }
    pub(crate) async fn delete(&self, path: &str) -> Result<(), RuntimeError> {
        let (status, body) = self
            .request(http::Method::DELETE, path, None::<&()>)
            .await?;
        if !status.is_success() {
            return Err(Self::status_error(status, &body));
        }
        Ok(())
    }
    pub(crate) async fn websocket(&self, path: &str) -> Result<RuntimeSocket, RuntimeError> {
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
        request.headers_mut().insert(
            http::header::ACCEPT_LANGUAGE,
            http::HeaderValue::from_static(aow_i18n::current_language().tag()),
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
    /// Persist this caller's node connection for its independent scheduling helper.
    pub fn write_connection_file(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        use std::{
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let endpoint = self.resolved_endpoint()?;
        let parent = path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Connection file needs a parent",
            )
        })?;
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        let temporary = parent.join(format!(".runtime-client.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec(
                &serde_json::json!({"url":endpoint.url.as_str(),"token":endpoint.token}),
            )?)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)?;
            Ok::<_, RuntimeError>(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
    pub(crate) async fn stream_request(
        &self,
        method: http::Method,
        path: &str,
        headers: http::HeaderMap,
        body: Option<reqwest::Body>,
    ) -> Result<reqwest::Response, RuntimeError> {
        let endpoint = self.resolved_endpoint()?;
        let mut request = self
            .http
            .request(method, self.url(&endpoint, path))
            .headers(headers)
            .bearer_auth(&endpoint.token)
            .header(
                http::header::ACCEPT_LANGUAGE,
                aow_i18n::current_language().tag(),
            )
            .timeout(Duration::from_secs(3600));
        if let Some(body) = body {
            request = request.body(body);
        }
        Ok(request.send().await?)
    }
    pub async fn close_socket(mut socket: RuntimeSocket) {
        let _ = tokio::time::timeout(Duration::from_secs(5), socket.close(None)).await;
    }
}
