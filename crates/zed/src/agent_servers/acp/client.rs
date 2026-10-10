//! Zed agent_servers/acp client dispatch, independent of GPUI and terminald.
use crate::{
    acp_thread::{AcpThread, history::ThreadStore},
    facade::{AcpHost, Permission},
};
use agent_client_protocol::{Responder, UntypedMessage};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

type PendingReplies = HashMap<String, (String, Permission, oneshot::Sender<Value>)>;
pub(crate) struct ClientContext {
    pub host: Arc<dyn AcpHost>,
    pub cwd: PathBuf,
    pub threads: Arc<Mutex<ThreadStore>>,
    pub sessions: Mutex<HashMap<String, String>>,
    pub pending_updates: Mutex<HashMap<String, Vec<Value>>>,
    pub replies: Mutex<PendingReplies>,
    pub stop: CancellationToken,
}
impl ClientContext {
    pub(crate) fn unregister_session(&self, remote_id: &str) {
        self.cancel_permissions(remote_id);
        self.sessions.lock().unwrap().remove(remote_id);
    }
    pub(crate) fn register_session(&self, remote_id: &str, id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(remote_id.into(), id.into());
        if let Some(updates) = self.pending_updates.lock().unwrap().remove(remote_id) {
            self.threads.lock().unwrap().update(id, |thread| {
                for update in updates {
                    AcpThread::handle_session_update(thread, update);
                }
            })?;
        }
        Ok(())
    }
    pub(super) fn handle_notification(&self, message: UntypedMessage) -> Result<()> {
        if message.method == "session/update" {
            let remote_id = message.params["sessionId"]
                .as_str()
                .context("Missing session ID")?;
            let sessions = self.sessions.lock().unwrap();
            if let Some(id) = sessions.get(remote_id) {
                self.threads.lock().unwrap().update(id, |thread| {
                    AcpThread::handle_session_update(thread, message.params["update"].clone())
                })?;
            } else {
                let mut pending = self.pending_updates.lock().unwrap();
                ensure!(
                    pending.len() < 100 || pending.contains_key(remote_id),
                    "Too many unregistered ACP sessions"
                );
                let updates = pending.entry(remote_id.into()).or_default();
                ensure!(updates.len() < 100000, "ACP replay exceeds event limit");
                updates.push(message.params["update"].clone());
            }
        } else if message.method == "elicitation/complete" {
            let ids: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
            let mut threads = self.threads.lock().unwrap();
            for id in ids {
                threads.update(&id, |thread| {
                    AcpThread::push(thread, "notice", message.params.clone())
                })?;
            }
        }
        Ok(())
    }
    pub(super) fn handle_request(
        self: &Arc<Self>,
        message: UntypedMessage,
        responder: Responder<Value>,
    ) {
        let context = self.clone();
        // The SDK dispatcher must remain free to process cancel, updates, and responses.
        tokio::spawn(async move {
            let result = context.dispatch(message).await;
            let response = match result {
                Ok(value) => responder.respond(value),
                Err(error) => responder.respond_with_error(
                    error
                        .downcast::<agent_client_protocol::Error>()
                        .unwrap_or_else(|error| {
                            agent_client_protocol::Error::internal_error().data(error.to_string())
                        }),
                ),
            };
            if let Err(error) = response {
                tracing::debug!(%error, "ACP response channel closed");
            }
        });
    }
    async fn dispatch(&self, message: UntypedMessage) -> Result<Value> {
        let params = message.params;
        match message.method.as_str() {
            "fs/read_text_file" => {
                self.require_session(&params)?;
                let request: agent_client_protocol::schema::v1::ReadTextFileRequest =
                    serde_json::from_value(params)?;
                let path = Path::new(&request.path);
                ensure!(path.is_absolute(), "ACP file paths must be absolute");
                let text = self.host.read_text_file(&self.cwd, path).await?;
                let content = if request.line.is_some() || request.limit.is_some() {
                    text.lines()
                        .skip(request.line.unwrap_or(1).saturating_sub(1) as usize)
                        .take(request.limit.unwrap_or(u32::MAX) as usize)
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    text
                };
                Ok(json!({"content":content}))
            }
            "fs/write_text_file" => {
                self.require_session(&params)?;
                let request: agent_client_protocol::schema::v1::WriteTextFileRequest =
                    serde_json::from_value(params)?;
                ensure!(
                    request.path.is_absolute(),
                    "ACP file paths must be absolute"
                );
                self.host
                    .write_text_file(&self.cwd, &request.path, &request.content)
                    .await?;
                Ok(json!({}))
            }
            "session/request_permission" | "elicitation/create" => {
                let kind = if message.method == "session/request_permission" {
                    let _: agent_client_protocol::schema::v1::RequestPermissionRequest =
                        serde_json::from_value(params.clone())?;
                    "permission"
                } else {
                    let _: agent_client_protocol::schema::v1::CreateElicitationRequest =
                        serde_json::from_value(params.clone())?;
                    "elicitation"
                };
                let remote_id = params["sessionId"]
                    .as_str()
                    .or_else(|| params["scope"]["sessionId"].as_str())
                    .unwrap_or("")
                    .to_owned();
                let id = self.sessions.lock().unwrap().get(&remote_id).cloned();
                ensure!(
                    remote_id.is_empty() || id.is_some(),
                    "Unknown ACP session for request"
                );
                let request_id = uuid::Uuid::new_v4().to_string();
                let permission = Permission {
                    id: request_id.clone(),
                    kind: kind.into(),
                    request: params,
                };
                let (sender, received) = oneshot::channel();
                self.replies
                    .lock()
                    .unwrap()
                    .insert(request_id.clone(), (remote_id, permission.clone(), sender));
                if let Some(id) = &id {
                    self.threads
                        .lock()
                        .unwrap()
                        .update(id, |thread| thread.permissions.push(permission))?;
                }
                let cancelled = || {
                    if kind == "permission" {
                        json!({"outcome":{"outcome":"cancelled"}})
                    } else {
                        json!({"action":"cancel"})
                    }
                };
                let response = tokio::select! { response = received => response.unwrap_or_else(|_| cancelled()), () = self.stop.cancelled() => cancelled() };
                self.replies.lock().unwrap().remove(&request_id);
                if let Some(id) = &id {
                    self.threads.lock().unwrap().update(id, |thread| {
                        thread
                            .permissions
                            .retain(|request| request.id != request_id)
                    })?;
                }
                Ok(response)
            }
            _ => Err(agent_client_protocol::Error::method_not_found().into()),
        }
    }
    fn require_session(&self, params: &Value) -> Result<()> {
        let id = params["sessionId"].as_str().context("Missing session ID")?;
        ensure!(
            self.sessions.lock().unwrap().contains_key(id),
            "Unknown ACP session"
        );
        Ok(())
    }
    pub(crate) fn requests(&self) -> Vec<Permission> {
        self.replies
            .lock()
            .unwrap()
            .values()
            .map(|(_, permission, _)| permission.clone())
            .collect()
    }
    pub(crate) fn answer(&self, request_id: &str, response: Value) -> Result<()> {
        let mut replies = self.replies.lock().unwrap();
        let (_, permission, _) = replies
            .get(request_id)
            .context("Request is no longer pending")?;
        if permission.kind == "permission" {
            let _: agent_client_protocol::schema::v1::RequestPermissionResponse =
                serde_json::from_value(response.clone())?;
            if response["outcome"]["outcome"] == "selected" {
                ensure!(
                    permission.request["options"]
                        .as_array()
                        .is_some_and(|options| options
                            .iter()
                            .any(|option| option["optionId"] == response["outcome"]["optionId"])),
                    "Unknown permission option"
                );
            }
        } else {
            let _: agent_client_protocol::schema::v1::CreateElicitationResponse =
                serde_json::from_value(response.clone())?;
        }
        let (_, _, sender) = replies
            .remove(request_id)
            .context("Request is no longer pending")?;
        sender
            .send(response)
            .map_err(|_| anyhow::anyhow!("Request cancelled"))
    }
    pub(super) fn cancel_permissions(&self, remote_id: &str) {
        self.replies
            .lock()
            .unwrap()
            .retain(|_, (session, _, _)| session != remote_id);
    }
    pub(super) fn disconnected(&self, reason: &str) {
        let ids: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
        self.replies.lock().unwrap().clear();
        let mut threads = self.threads.lock().unwrap();
        for id in ids {
            if let Err(error) = threads.update(&id, |thread| {
                thread.status = "disconnected".into();
                thread.error = Some(reason.into());
                thread.permissions.clear();
            }) {
                tracing::error!(%error, "Failed to save ACP disconnect");
            }
        }
    }
}
