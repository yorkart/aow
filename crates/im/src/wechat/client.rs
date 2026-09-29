use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::Message;
use anyhow::{Context, Result, ensure};

use super::{
    api::{self, Api},
    model::{
        ConnectionStatus, Credentials, Inner, Session, TestReceipt, TestVerification, WechatClient,
    },
};

impl Credentials {
    pub fn validate(&self) -> Result<()> {
        api::validate_base(&self.base_url)?;
        ensure!(
            aow_id::Snowflake::from_base36(&self.binding_id).is_ok(),
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
        let session = self.inner.session.lock().unwrap();
        status.context_ready = session.context_token.is_some();
        status.verification = session.verification.clone();
        status
    }

    /// Persist setup progress for this binding; accepting a send is not a receipt.
    pub async fn send_test(&self, message: &Message) -> Result<TestVerification> {
        let _sending = self.inner.sending.lock().await;
        self.inner
            .update_session(|session| session.verification = None)?;
        let verification = TestVerification {
            test_id: aow_id::new_id(),
            receipt: TestReceipt::Sent,
        };
        self.inner.send(message, &verification.test_id).await?;
        self.inner.update_session(|session| {
            session.verification = Some(verification.clone());
        })?;
        Ok(verification)
    }

    pub async fn record_test_receipt(
        &self,
        test_id: &str,
        received: bool,
    ) -> Result<TestVerification> {
        let _sending = self.inner.sending.lock().await;
        // Serialize with test sends, so an old page cannot confirm a newer test
        // or a replacement binding (even when it belongs to the same account).
        let mut verification = self
            .inner
            .session
            .lock()
            .unwrap()
            .verification
            .clone()
            .filter(|verification| verification.test_id == test_id)
            .context("测试记录已更新，请刷新微信设置后重试")?;
        verification.receipt = if received {
            TestReceipt::Confirmed
        } else {
            TestReceipt::Missing
        };
        self.inner.update_session(|session| {
            session.verification = Some(verification.clone());
        })?;
        Ok(verification)
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
                    verification: None,
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
    pub(super) fn update_session(&self, update: impl FnOnce(&mut Session)) -> Result<()> {
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
