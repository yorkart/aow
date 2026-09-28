use std::{future::Future, sync::Arc};

use crate::{Message, feishu, wechat};

pub trait ImProvider: Send + Sync {
    fn send(
        &self,
        message: &Message,
        delivery_id: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[derive(Clone)]
pub enum Provider {
    Feishu(Arc<feishu::FeishuClient>),
    Wechat(Arc<wechat::WechatClient>),
}

impl Provider {
    pub fn start(&self) {
        if let Self::Wechat(client) = self {
            client.start();
        }
    }

    pub fn retire(&self) {
        if let Self::Wechat(client) = self {
            client.retire();
        }
    }
}

impl ImProvider for Provider {
    async fn send(&self, message: &Message, delivery_id: &str) -> anyhow::Result<()> {
        match self {
            Self::Feishu(client) => client.send(message, delivery_id).await,
            Self::Wechat(client) => client.send(message, delivery_id).await,
        }
    }
}
