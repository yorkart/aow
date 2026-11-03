use super::{
    LocalClientError,
    response::{expect_empty, expect_json},
    transport,
};
use bytes::Bytes;
use http::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;
#[derive(Clone, Debug)]
pub struct LocalClient {
    socket: PathBuf,
}
impl LocalClient {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket
    }
    /// Local HTTP/JSON transport, also used with the AoW CLI socket.
    pub async fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R, LocalClientError> {
        let (status, body) = self.request(Method::GET, path, None::<&()>).await?;
        expect_json(status, body, &[StatusCode::OK])
    }

    pub async fn post_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, LocalClientError> {
        let (status, body) = self.request(Method::POST, path, Some(request)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn put_json<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<R, LocalClientError> {
        let (status, body) = self.request(Method::PUT, path, Some(request)).await?;
        expect_json(status, body, &[StatusCode::OK, StatusCode::CREATED])
    }

    pub async fn delete_json<T: Serialize + ?Sized>(
        &self,
        path: &str,
        request: &T,
    ) -> Result<(), LocalClientError> {
        let (status, body) = self.request(Method::DELETE, path, Some(request)).await?;
        expect_empty(status, body, &[StatusCode::NO_CONTENT])
    }

    async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<(StatusCode, Bytes), LocalClientError> {
        transport::request(&self.socket, method, path, body).await
    }
}
