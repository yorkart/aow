use serde_json::Value;
use std::{
    io,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};

use super::{
    super::{config::MAX_JS_SAFE_INTEGER, rpc::RpcError, session::SessionState},
    protocol::{
        MAX_FRAME_BYTES, MAX_REQUEST_METADATA_BYTES, PROTOCOL_VERSION, Response, read_frame,
    },
};

type Sessions = Arc<Mutex<Vec<Weak<SessionState>>>>;

pub(in crate::vt_worker) struct RpcClient {
    stdin: ChildStdin,
    pub(in crate::vt_worker) responses: mpsc::Receiver<Result<Response, RpcError>>,
    reader: JoinHandle<()>,
    next_id: u64,
    request_timeout: Duration,
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl RpcClient {
    pub(in crate::vt_worker) fn new(
        stdin: ChildStdin,
        stdout: ChildStdout,
        request_timeout: Duration,
        sessions: Sessions,
    ) -> Self {
        // Only one query is in flight. Mutation failures bypass this channel
        // and invalidate their session directly; they cannot fill a reply
        // queue while the actor is blocked writing to a full pipe.
        let (sender, responses) = mpsc::channel(1);
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            loop {
                let response = match read_frame(&mut stdout).await {
                    Ok(response) => response,
                    Err(error) => {
                        let _ = sender.send(Err(RpcError::Io(error))).await;
                        break;
                    }
                };
                if matches!(
                    response.metadata.get("action").and_then(Value::as_str),
                    Some("write" | "resize")
                ) {
                    if let Err(error) = mutation_error(response, &sessions) {
                        let _ = sender.send(Err(error)).await;
                        break;
                    }
                } else if sender.send(Ok(response)).await.is_err() {
                    break;
                }
            }
        });
        Self {
            stdin,
            responses,
            reader,
            next_id: 1,
            request_timeout,
        }
    }

    pub(in crate::vt_worker) async fn send(
        &mut self,
        action: &str,
        mut request: Value,
        data: &[u8],
    ) -> Result<u64, RpcError> {
        let id = self.next_id;
        if id > MAX_JS_SAFE_INTEGER {
            return Err(RpcError::Protocol(
                "VT request ID exceeds JavaScript safe integer range".to_owned(),
            ));
        }
        self.next_id += 1;
        let object = request
            .as_object_mut()
            .ok_or_else(|| RpcError::Protocol("request params must be an object".to_owned()))?;
        object.insert("id".to_owned(), id.into());
        object.insert("action".to_owned(), action.into());
        object.insert("protocol_version".to_owned(), PROTOCOL_VERSION.into());
        let metadata =
            serde_json::to_vec(&request).map_err(|e| RpcError::Protocol(e.to_string()))?;
        let frame_length = 4 + metadata.len() + data.len();
        if metadata.len() > MAX_REQUEST_METADATA_BYTES || frame_length > MAX_FRAME_BYTES {
            return Err(RpcError::Protocol(
                "VT request exceeds frame size limit".to_owned(),
            ));
        }
        let mut prefix = [0; 8];
        prefix[..4].copy_from_slice(&(frame_length as u32).to_be_bytes());
        prefix[4..].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
        timeout(self.request_timeout, async {
            self.stdin.write_all(&prefix).await?;
            self.stdin.write_all(&metadata).await?;
            self.stdin.write_all(data).await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| RpcError::Timeout)?
        .map_err(RpcError::Io)?;
        Ok(id)
    }

    pub(in crate::vt_worker) async fn call(
        &mut self,
        action: &str,
        request: Value,
    ) -> Result<Response, RpcError> {
        let session = request.get("session_id").cloned();
        let generation = request.get("generation").cloned();
        let id = self.send(action, request, &[]).await?;
        let mut response = timeout(self.request_timeout, self.responses.recv())
            .await
            .map_err(|_| RpcError::Timeout)?
            .ok_or_else(|| {
                RpcError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "VT response stream closed",
                ))
            })??;
        let header = &mut response.metadata;
        if header.get("id").and_then(Value::as_u64) != Some(id)
            || header.get("action").and_then(Value::as_str) != Some(action)
        {
            return Err(RpcError::Protocol(
                "VT response does not match request".to_owned(),
            ));
        }
        if action != "snapshot" && !response.data.is_empty() {
            return Err(RpcError::Protocol(
                "unexpected VT response payload".to_owned(),
            ));
        }
        match header.get("ok").and_then(Value::as_bool) {
            Some(true) => {
                let result = header
                    .as_object_mut()
                    .and_then(|header| header.remove("result"))
                    .ok_or_else(|| {
                        RpcError::Protocol("successful response omitted result".to_owned())
                    })?;
                if result.get("session_id") != session.as_ref()
                    || result.get("generation") != generation.as_ref()
                {
                    return Err(RpcError::Protocol(
                        "VT response session/generation mismatch".to_owned(),
                    ));
                }
                response.metadata = result;
                Ok(response)
            }
            Some(false) => Err(RpcError::Worker(error_message(header))),
            None => Err(RpcError::Protocol(
                "VT response omitted boolean ok".to_owned(),
            )),
        }
    }
}

fn error_message(value: &Value) -> String {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or("worker_error");
    let message = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("VT worker rejected operation");
    format!("{code}: {message}")
}

fn mutation_error(response: Response, sessions: &Sessions) -> Result<(), RpcError> {
    let header = response.metadata;
    if header.get("ok").and_then(Value::as_bool) != Some(false) || !response.data.is_empty() {
        return Err(RpcError::Protocol(
            "one-way VT mutation returned an unexpected reply".to_owned(),
        ));
    }
    let session_id = header
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::Protocol("mutation error omitted session_id".to_owned()))?;
    let generation = header
        .get("generation")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::Protocol("mutation error omitted generation".to_owned()))?;
    let sessions = sessions
        .lock()
        .map_err(|_| RpcError::Protocol("VT session registry poisoned".to_owned()))?;
    if let Some(session) = sessions
        .iter()
        .filter_map(Weak::upgrade)
        .find(|session| session.session_id == session_id && session.generation == generation)
    {
        session.fail();
        tracing::warn!(%session_id, error = %error_message(&header), "VT worker rejected mutation; using raw replay");
    }
    Ok(())
}
