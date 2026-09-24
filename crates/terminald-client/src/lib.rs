use std::path::{Path, PathBuf};

pub use aow_protocol::{
    ApiError, TerminalAgentList, TerminalAttachClientMessage, TerminalAttachServerMessage,
    TerminalControlState, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec,
    TerminalRuntimeStatus, TerminaldHealth,
};
use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::net::UnixStream;
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite};

// RFC 3986 unreserved characters are the only bytes allowed literally in a
// runtime ID path segment. This prevents reserved delimiters from changing
// either HTTP routing or WebSocket request-target semantics.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub type TerminaldAttachStream = WebSocketStream<UnixStream>;

#[derive(Debug, Error)]
pub enum TerminaldClientError {
    #[error("terminald I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminald HTTP transport error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("failed to construct terminald HTTP request: {0}")]
    Http(#[from] http::Error),
    #[error("invalid terminald JSON response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("terminald WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("terminald returned HTTP {status}: {message}")]
    HttpStatus {
        status: StatusCode,
        message: String,
        error: Option<ApiError>,
    },
}

#[derive(Debug, Clone)]
pub struct TerminaldClient {
    socket: PathBuf,
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

    /// Local HTTP/JSON transport, also used with the AOW CLI socket.
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

    pub async fn attach(&self, id: &str) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume(id, None, None).await
    }

    pub async fn attach_from(
        &self,
        id: &str,
        epoch: &str,
        after: u64,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume(id, Some(epoch), Some(after)).await
    }

    pub async fn attach_with_resume(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_capabilities(id, epoch, after, false)
            .await
    }

    /// Connect using legacy auto-claim semantics while explicitly negotiating
    /// geometry-aware VT snapshot restores. This is primarily useful for
    /// direct terminald clients and end-to-end compatibility tests.
    pub async fn attach_with_resume_capabilities(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, false, vt_snapshot, false)
            .await
    }

    /// Connect using the opt-in controller protocol. Unlike the legacy attach
    /// endpoint behavior, this connection stays neutral until it sends a
    /// `claim` control message.
    pub async fn attach_controlled_with_resume(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_controlled_with_resume_capabilities(id, epoch, after, false)
            .await
    }

    /// Connect using controller v2 and explicitly negotiate support for the
    /// geometry-aware VT snapshot restore extension. Existing callers remain
    /// on raw replay unless they opt in here.
    pub async fn attach_controlled_with_resume_capabilities(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, true, vt_snapshot, false)
            .await
    }

    /// Connect using controller v2 and opt into read-only output subscriptions
    /// when another attachment already owns terminal input.
    pub async fn attach_controlled_with_resume_capabilities_and_observer(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
        observer: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, true, vt_snapshot, observer)
            .await
    }

    async fn attach_with_resume_mode(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        controlled: bool,
        vt_snapshot: bool,
        observer: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        let stream = UnixStream::connect(&self.socket).await?;
        let (socket, _) = client_async(
            attach_uri(id, epoch, after, controlled, vt_snapshot, observer),
            stream,
        )
        .await?;
        Ok(socket)
    }

    pub async fn connect_attach(
        &self,
        id: &str,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach(id).await
    }

    async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<(StatusCode, Bytes), TerminaldClientError> {
        let stream = UnixStream::connect(&self.socket).await?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing_fallback(&error);
            }
        });

        let body = match body {
            Some(body) => Bytes::from(serde_json::to_vec(body)?),
            None => Bytes::new(),
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "aow-terminald");
        if !body.is_empty() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let response = sender.send_request(builder.body(Full::new(body))?).await?;
        let status = response.status();
        let body = response.into_body().collect().await?.to_bytes();
        Ok((status, body))
    }
}

impl Default for TerminaldClient {
    fn default() -> Self {
        Self::default_socket()
    }
}

pub fn default_socket_path() -> PathBuf {
    if let Some(path) = nonempty_env("AOW_TERMINALD_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = nonempty_env("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir)
            .join("aow-terminald")
            .join("terminald.sock");
    }
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/aow-terminald-{uid}/terminald.sock"))
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn runtime_path(id: &str) -> String {
    format!("/v1/runtimes/{}", encode_id(id))
}

fn attach_uri(
    id: &str,
    epoch: Option<&str>,
    after: Option<u64>,
    controlled: bool,
    vt_snapshot: bool,
    observer: bool,
) -> String {
    let uri = format!("ws://aow-terminald/v1/runtimes/{}/attach", encode_id(id));
    let mut uri = match (epoch, after) {
        (Some(epoch), Some(after)) => {
            format!("{uri}?epoch={}&after={after}", encode_id(epoch))
        }
        _ => uri,
    };
    if controlled {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}control=v2");
    }
    if vt_snapshot {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}capabilities=vt-snapshot-v1");
    }
    if observer {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}observer=v1");
    }
    uri
}

fn encode_id(id: &str) -> String {
    utf8_percent_encode(id, PATH_SEGMENT).to_string()
}

fn expect_json<T: DeserializeOwned>(
    status: StatusCode,
    body: Bytes,
    expected: &[StatusCode],
) -> Result<T, TerminaldClientError> {
    if !expected.contains(&status) {
        return Err(status_error(status, &body));
    }
    Ok(serde_json::from_slice(&body)?)
}

fn expect_empty(
    status: StatusCode,
    body: Bytes,
    expected: &[StatusCode],
) -> Result<(), TerminaldClientError> {
    if expected.contains(&status) {
        Ok(())
    } else {
        Err(status_error(status, &body))
    }
}

fn status_error(status: StatusCode, body: &[u8]) -> TerminaldClientError {
    let error = serde_json::from_slice::<ApiError>(body).ok();
    let message = error
        .as_ref()
        .map(|error| error.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    TerminaldClientError::HttpStatus {
        status,
        message,
        error,
    }
}

fn tracing_fallback(error: &hyper::Error) {
    // Request completion owns the meaningful error path; a late connection
    // driver error must not panic applications that use this small client.
    let _ = error;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_ids_are_encoded_as_one_path_segment() {
        assert_eq!(runtime_path("a/b c"), "/v1/runtimes/a%2Fb%20c");
        assert_eq!(
            encode_id("aZ09-._~:/?#[]@!$&'()*+,;=%é"),
            "aZ09-._~%3A%2F%3F%23%5B%5D%40%21%24%26%27%28%29%2A%2B%2C%3B%3D%25%C3%A9"
        );
    }

    #[test]
    fn attach_uri_encodes_the_id_and_optional_offset() {
        assert_eq!(
            attach_uri("a/b c", Some("epoch/one"), Some(42), false, false, false),
            "ws://aow-terminald/v1/runtimes/a%2Fb%20c/attach?epoch=epoch%2Fone&after=42"
        );
        assert_eq!(
            attach_uri("runtime", None, None, false, false, false),
            "ws://aow-terminald/v1/runtimes/runtime/attach"
        );
        assert_eq!(
            attach_uri("runtime", None, Some(42), false, false, false),
            "ws://aow-terminald/v1/runtimes/runtime/attach"
        );
        assert_eq!(
            attach_uri("runtime", None, None, true, false, false),
            "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2"
        );
        assert_eq!(
            attach_uri("runtime", Some("epoch"), Some(42), true, false, false),
            "ws://aow-terminald/v1/runtimes/runtime/attach?epoch=epoch&after=42&control=v2"
        );
        assert_eq!(
            attach_uri("runtime", None, None, true, true, false),
            "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2&capabilities=vt-snapshot-v1"
        );
        assert_eq!(
            attach_uri("runtime", None, None, true, true, true),
            "ws://aow-terminald/v1/runtimes/runtime/attach?control=v2&capabilities=vt-snapshot-v1&observer=v1"
        );
    }
}
