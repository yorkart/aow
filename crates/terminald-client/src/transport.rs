use std::path::Path;

use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::UnixStream;

use super::TerminaldClientError;

pub(super) async fn request<T: Serialize + ?Sized>(
    socket: &Path,
    method: Method,
    path: &str,
    body: Option<&T>,
) -> Result<(StatusCode, Bytes), TerminaldClientError> {
    let stream = UnixStream::connect(socket).await?;
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

fn tracing_fallback(error: &hyper::Error) {
    // Request completion owns the meaningful error path; a late connection
    // driver error must not panic applications that use this small client.
    let _ = error;
}
