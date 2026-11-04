use super::{
    ServerClientError,
    response::{expect_empty, expect_json},
    transport,
};
use bytes::Bytes;
use http::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;
#[derive(Clone)]
pub struct ServerClient {
    socket: PathBuf,
    remote: Option<(reqwest::Url, String)>,
}
impl ServerClient {
    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            remote: None,
        }
    }
    pub fn remote(url: &str, token: String) -> anyhow::Result<Self> {
        let mut url = reqwest::Url::parse(url)?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "Invalid Server URL"
        );
        anyhow::ensure!(
            !token.is_empty() && token.bytes().all(|c| c.is_ascii_graphic()),
            "Invalid Server token"
        );
        let base = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base}/api/cli"));
        Ok(Self {
            socket: PathBuf::new(),
            remote: Some((url, token)),
        })
    }
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }
    pub fn address(&self) -> String {
        self.remote.as_ref().map_or_else(
            || self.socket.display().to_string(),
            |(url, _)| url.to_string(),
        )
    }
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket
    }
    /// Local HTTP/JSON transport, also used with the AoW CLI socket.
    pub async fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R, ServerClientError> {
        let (status, body) = self.request(Method::GET, path, None::<&()>).await?;
        expect_json(status, body, &[StatusCode::OK])
    }

    pub async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, ServerClientError> {
        let (status, body) = self.request(Method::POST, path, Some(request)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn put_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, ServerClientError> {
        let (status, body) = self.request(Method::PUT, path, Some(request)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn delete_json<T: Serialize + ?Sized>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<(), ServerClientError> {
        let (status, body) = self.request(Method::DELETE, path, Some(request)).await?;
        expect_empty(status, body, &[StatusCode::NO_CONTENT])
    }

    async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<(StatusCode, Bytes), ServerClientError> {
        if let Some((base, token)) = &self.remote {
            let mut url = base.clone();
            let (path, query) = path
                .split_once('?')
                .map_or((path, None), |(p, q)| (p, Some(q)));
            url.set_path(&format!("{}{path}", base.path()));
            url.set_query(query);
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()?;
            let mut request = client.request(method, url).bearer_auth(token);
            if let Some(body) = body {
                request = request.json(body);
            }
            let mut response = request
                .send()
                .await
                .map_err(|error| ServerClientError::Remote(error.without_url()))?;
            let status = response.status();
            let mut bytes = bytes::BytesMut::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len().saturating_add(chunk.len()) > 64 * 1024 * 1024 {
                    return Err(ServerClientError::Io(std::io::Error::other(
                        "Server response exceeds limit",
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok((status, bytes.freeze()));
        }
        transport::request(&self.socket, method, path, body).await
    }
}

impl std::fmt::Debug for ServerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerClient")
            .field("address", &self.address())
            .finish_non_exhaustive()
    }
}
