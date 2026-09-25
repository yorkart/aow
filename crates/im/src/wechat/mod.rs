//! Tencent iLink transport, with Hermes-style one-shot sends and context fallback.
//! No dependency on a browser, desktop WeChat, OpenClaw or the AoW server.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{ImProvider, Message};
use api::{Api, check, stale_context};

mod api;
mod login;
pub use login::{LoginManager, LoginView};

// No Debug: these values are local secrets, never part of the settings view.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub account_id: String,
    pub user_id: String,
    pub base_url: String,
    pub bot_token: String,
    pub binding_id: String,
}

impl Credentials {
    pub fn validate(&self) -> Result<()> {
        api::validate_base(&self.base_url)?;
        ensure!(
            Uuid::parse_str(&self.binding_id).is_ok(),
            "无效的微信绑定 ID"
        );
        for value in [&self.account_id, &self.user_id, &self.bot_token] {
            ensure!(
                !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control),
                "微信凭证不完整或无效"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Session {
    cursor: String,
    context_token: Option<String>,
}

#[derive(Clone, Default, Serialize)]
pub struct ConnectionStatus {
    pub receiving: bool,
    pub context_ready: bool,
    pub error: Option<String>,
}

struct Inner {
    active: AtomicBool,
    api: Api,
    credentials: Credentials,
    path: Option<PathBuf>,
    session: Mutex<Session>,
    status: Mutex<ConnectionStatus>,
    sending: tokio::sync::Mutex<()>,
}

pub struct WechatClient {
    inner: Arc<Inner>,
    receiver: Mutex<Option<JoinHandle<()>>>,
}

impl WechatClient {
    pub fn new(credentials: Credentials, state_dir: Option<&Path>) -> Result<Self> {
        credentials.validate()?;
        let path = state_dir.map(|root| {
            root.join("im/wechat")
                .join(format!("{}.json", credentials.binding_id))
        });
        let session = match path.as_ref().map(std::fs::read) {
            Some(Ok(bytes)) => serde_json::from_slice(&bytes).context("无法读取微信会话状态")?,
            Some(Err(error)) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            _ => Session::default(),
        };
        Ok(Self {
            inner: Arc::new(Inner {
                active: AtomicBool::new(true),
                api: Api::new()?,
                credentials,
                path,
                session: Mutex::new(session),
                status: Mutex::new(ConnectionStatus::default()),
                sending: tokio::sync::Mutex::new(()),
            }),
            receiver: Mutex::new(None),
        })
    }

    pub fn status(&self) -> ConnectionStatus {
        let mut status = self.inner.status.lock().unwrap().clone();
        status.context_ready = self.inner.session.lock().unwrap().context_token.is_some();
        status
    }

    /// Receive polling is optional for sending and is owned by this client.
    pub fn start(&self) {
        if !self.inner.active.load(Ordering::Acquire) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mut receiver = self.receiver.lock().unwrap();
        if !self.inner.active.load(Ordering::Acquire)
            || receiver.as_ref().is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let inner = self.inner.clone();
        *receiver = Some(runtime.spawn(async move {
            loop {
                let result = inner.receive().await;
                let failed = result.is_err();
                *inner.status.lock().unwrap() = ConnectionStatus {
                    receiving: !failed,
                    context_ready: false,
                    error: result.err().map(|error| error.to_string()),
                };
                // Long polls normally hold upstream for ~35s; enforce pacing when
                // it returns immediately, and back off on transient failures.
                tokio::time::sleep(Duration::from_secs(if failed { 15 } else { 1 })).await;
            }
        }));
    }

    pub fn stop(&self) {
        if let Some(task) = self.receiver.lock().unwrap().take() {
            task.abort();
        }
        self.inner.status.lock().unwrap().receiving = false;
    }

    /// Retire a replaced/removed binding, including any outstanding clones.
    /// An already accepted remote HTTP request cannot be recalled.
    pub fn retire(&self) {
        self.inner.active.store(false, Ordering::Release);
        self.stop();
        let mut session = self.inner.session.lock().unwrap();
        *session = Session::default();
        if let Some(path) = &self.inner.path
            && let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "cannot remove retired WeChat context");
        }
    }
}

impl Drop for WechatClient {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Inner {
    fn update_session(&self, update: impl FnOnce(&mut Session)) -> Result<()> {
        let mut session = self.session.lock().unwrap();
        ensure!(self.active.load(Ordering::Acquire), "微信绑定已移除或替换");
        let mut next = session.clone();
        update(&mut next);
        if next == *session {
            return Ok(());
        }
        if let Some(path) = &self.path {
            save_session(path, &next)?;
        }
        *session = next;
        Ok(())
    }

    async fn receive(&self) -> Result<()> {
        let cursor = self.session.lock().unwrap().cursor.clone();
        let data = self
            .api
            .post(
                &self.credentials.base_url,
                "/ilink/bot/getupdates",
                Some(&self.credentials.bot_token),
                json!({"get_updates_buf":cursor}),
                Duration::from_secs(40),
            )
            .await?;
        check(&data)?;
        self.update_session(|session| {
            if let Some(cursor) = data["get_updates_buf"]
                .as_str()
                .filter(|cursor| !cursor.is_empty())
            {
                session.cursor = cursor.to_owned();
            }
            for message in data["msgs"].as_array().into_iter().flatten() {
                // Only the QR scanner is a recipient. Other users and group
                // messages must never change the destination or its context.
                if message["from_user_id"].as_str() == Some(&self.credentials.user_id)
                    && message["message_type"].as_i64() == Some(1)
                    && message["group_id"].as_str().is_none_or(str::is_empty)
                    && let Some(token) = message["context_token"]
                        .as_str()
                        .filter(|token| !token.is_empty())
                {
                    session.context_token = Some(token.to_owned());
                }
            }
        })
    }
}

impl ImProvider for WechatClient {
    async fn send(&self, message: &Message, delivery_id: &str) -> Result<()> {
        let inner = &self.inner;
        let _sending = inner.sending.lock().await;
        ensure!(inner.active.load(Ordering::Acquire), "微信绑定已移除或替换");
        let context = inner.session.lock().unwrap().context_token.clone();
        let mut body = json!({"msg": {
            "from_user_id":"", "to_user_id":inner.credentials.user_id,
            "client_id":delivery_id, "message_type":2, "message_state":2,
            "item_list":[{"type":1,"text_item":{"text":message.text(4000)}}]
        }});
        if let Some(token) = &context {
            body["msg"]["context_token"] = json!(token);
        }
        let data = inner
            .api
            .post(
                &inner.credentials.base_url,
                "/ilink/bot/sendmessage",
                Some(&inner.credentials.bot_token),
                body.clone(),
                Duration::from_secs(15),
            )
            .await?;
        if context.is_some() && stale_context(&data) {
            // Hermes fallback: only an explicit session rejection is safe to
            // retry, once, with the same delivery ID. Never retry a timeout.
            inner.update_session(|session| {
                // A new inbound context may have arrived while sending.
                if session.context_token == context {
                    session.context_token = None;
                }
            })?;
            body["msg"].as_object_mut().unwrap().remove("context_token");
            let retried = inner
                .api
                .post(
                    &inner.credentials.base_url,
                    "/ilink/bot/sendmessage",
                    Some(&inner.credentials.bot_token),
                    body,
                    Duration::from_secs(15),
                )
                .await?;
            check(&retried)
        } else {
            check(&data)
        }
    }
}

fn save_session(path: &Path, session: &Session) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("微信会话路径无效")?;
    std::fs::create_dir_all(parent)?;
    // NamedTempFile creates mode 0600; persist atomically replaces old content.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(session)?)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("无法保存微信会话"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
