//! Native ACP transport bookkeeping only; no session model or local adapter fallback.
use super::*;
use anyhow::{Result, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value>>>>>;
struct PendingRequest {
    id: String,
    pending: Pending,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.id);
    }
}
pub(super) struct Connection {
    writer: mpsc::Sender<Value>,
    pending: Pending,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl AcpClient {
    async fn native_request(&self, mut message: Value) -> Result<Value> {
        if message.get("method").is_some() {
            message["params"]["_meta"]["aow"]["language"] =
                json!(aow_i18n::current_language().tag());
        }
        let host = self.remote().await?;
        self.ensure_events(&host).await?;
        let mut guard = self.0.native.lock().await;
        if guard
            .as_ref()
            .is_none_or(|connection| connection.task.is_finished())
        {
            let mut socket = host.websocket("/v1/acp/conversations/acp").await?;
            let request = agent_client_protocol::schema::v1::InitializeRequest::new(
                agent_client_protocol::schema::ProtocolVersion::V1,
            );
            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":request}).to_string().into())).await?;
            let response = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await?
                .ok_or_else(|| anyhow::anyhow!("ACP connection closed during initialize"))??;
            ensure!(
                matches!(response, Message::Text(text) if serde_json::from_str::<Value>(&text)?["result"].is_object()),
                "ACP initialization failed"
            );
            let (writer, mut messages) = mpsc::channel::<Value>(128);
            let pending: Pending = Default::default();
            let awaiting = pending.clone();
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        message = messages.recv() => {
                            let Some(message) = message else { break; };
                            if socket.send(Message::Text(message.to_string().into())).await.is_err() { break; }
                        }
                        message = socket.next() => match message {
                            Some(Ok(Message::Text(text))) => {
                                let Ok(value) = serde_json::from_str::<Value>(&text) else { break; };
                                let key = if matches!(value["method"].as_str(), Some("_aow/admitted" | "_aow/answered")) { value["params"]["id"].as_str() }
                                    else if value.get("method").is_none() { value["id"].as_str() } else { None };
                                if let Some(sender) = key.and_then(|key| awaiting.lock().unwrap().remove(key)) {
                                    let result = if let Some(error) = value.get("error") {
                                        Err(client::RemoteOperationError(ServiceError { code: if error["code"] == -32000 { "auth_required" } else { "operation_failed" }.into(), message: error["data"].as_str().or_else(|| error["message"].as_str()).unwrap_or("ACP operation failed").into() }).into())
                                    } else { Ok(value.get("result").cloned().unwrap_or(Value::Null)) };
                                    let _ = sender.send(result);
                                }
                            }
                            Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() { break; } }
                            Some(Ok(Message::Pong(_))) => {}
                            _ => break,
                        }
                    }
                }
                for (_, sender) in std::mem::take(&mut *awaiting.lock().unwrap()) {
                    let _ = sender.send(Err(anyhow::anyhow!(
                        "ACP connection closed; operation outcome may be unknown"
                    )));
                }
            });
            *guard = Some(Connection {
                writer,
                pending,
                task,
            });
        }
        let connection = guard.as_ref().unwrap();
        let id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let notification = message["method"] == "session/cancel";
        if !notification {
            message["id"] = json!(id);
        }
        message["jsonrpc"] = json!("2.0");
        let (sender, receiver) = oneshot::channel();
        if !notification {
            connection
                .pending
                .lock()
                .unwrap()
                .insert(id.clone(), sender);
        }
        let _pending = PendingRequest {
            id: id.clone(),
            pending: connection.pending.clone(),
        };
        let writer = connection.writer.clone();
        drop(guard);
        writer.send(message).await?;
        if notification {
            return Ok(Value::Null);
        }
        let outcome = tokio::time::timeout(Duration::from_secs(120), receiver).await;
        outcome??
    }
    pub async fn start_prompt(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        let request = agent_client_protocol::schema::v2::PromptRequest::new(
            thread_id.to_owned(),
            serde_json::from_value(serde_json::to_value(content)?)?,
        );
        self.native_request(json!({"method":"session/prompt","params":request}))
            .await?;
        Ok(())
    }
    pub async fn start_prompt_once(
        &self,
        thread_id: &str,
        request_id: &str,
        content: Vec<Content>,
    ) -> Result<()> {
        let request = agent_client_protocol::schema::v2::PromptRequest::new(
            thread_id.to_owned(),
            serde_json::from_value(serde_json::to_value(content)?)?,
        );
        let mut params = serde_json::to_value(request)?;
        params["_meta"] = json!({"aow":{"requestId":request_id}});
        self.native_request(json!({"method":"session/prompt","params":params}))
            .await?;
        Ok(())
    }
    pub async fn cancel(&self, thread_id: &str) -> Result<()> {
        self.native_request(json!({"method":"session/cancel","params":agent_client_protocol::schema::v1::CancelNotification::new(thread_id.to_owned())})).await?;
        Ok(())
    }
    pub async fn start_steering(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        self.native_request(
            json!({"method":"_aow/steer","params":{"sessionId":thread_id,"prompt":content}}),
        )
        .await?;
        Ok(())
    }
    pub async fn set_mode(&self, thread_id: &str, mode: &str) -> Result<()> {
        self.native_request(json!({"method":"session/set_mode","params":agent_client_protocol::schema::v1::SetSessionModeRequest::new(thread_id.to_owned(),mode.to_owned())})).await?;
        Ok(())
    }
    pub async fn set_config_option(
        &self,
        thread_id: &str,
        option: &str,
        value: Value,
    ) -> Result<()> {
        self.native_request(json!({"method":"session/set_config_option","params":{"sessionId":thread_id,"configId":option,"value":value}})).await?;
        Ok(())
    }
    pub async fn authenticate(&self, connection_id: &str, method_id: &str) -> Result<()> {
        self.native_request(json!({"method":"authenticate","params":{"methodId":method_id,"_meta":{"aow":{"connectionId":connection_id}}}})).await?;
        Ok(())
    }
    pub async fn answer(&self, _thread_id: &str, request: &str, response: Value) -> Result<()> {
        self.native_request(json!({"id":request,"result":response}))
            .await?;
        Ok(())
    }
    pub async fn answer_connection(
        &self,
        _connection_id: &str,
        request_id: &str,
        response: Value,
    ) -> Result<()> {
        self.native_request(json!({"id":request_id,"result":response}))
            .await?;
        Ok(())
    }
}
