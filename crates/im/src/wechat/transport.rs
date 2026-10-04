use std::{sync::atomic::Ordering, time::Duration};

use anyhow::{Result, ensure};
use serde_json::json;

use super::{
    api::{check, stale_context},
    model::{Inner, WechatClient},
};
use crate::{ImProvider, Message};

impl Inner {
    pub(super) async fn receive(&self) -> Result<()> {
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
        let _sending = self.inner.sending.lock().await;
        self.inner.send(message, delivery_id).await
    }
}

impl Inner {
    pub(super) async fn send(&self, message: &Message, delivery_id: &str) -> Result<()> {
        let inner = self;
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
