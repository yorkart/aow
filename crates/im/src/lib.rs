//! IM transport implementations. Notification dispatch only sees this interface.
use std::{future::Future, sync::Arc};

use serde::{Deserialize, Serialize};

use std::path::Path;

mod message;
pub use message::{Field, Message};
pub mod wechat;

mod feishu;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImKind {
    Feishu,
    Wechat,
}

// Credentials intentionally have no Debug implementation and are never returned by APIs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImConfig {
    Feishu { app_id: String, app_secret: String },
    Wechat { credentials: wechat::Credentials },
}

#[derive(Serialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ImConfigView {
    Wechat {
        account_id: String,
        user_id: String,
    },
    Feishu {
        app_id: String,
        secret_configured: bool,
    },
}

#[derive(Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImConfigUpdate {
    // Only retain an existing binding. Credentials arrive through QR login.
    Wechat {},
    Feishu {
        app_id: String,
        app_secret: Option<String>,
    },
}

impl ImConfig {
    pub fn kind(&self) -> ImKind {
        match self {
            Self::Feishu { .. } => ImKind::Feishu,
            Self::Wechat { .. } => ImKind::Wechat,
        }
    }

    pub fn view(&self) -> ImConfigView {
        match self {
            Self::Wechat { credentials } => ImConfigView::Wechat {
                account_id: credentials.account_id.clone(),
                user_id: credentials.user_id.clone(),
            },
            Self::Feishu { app_id, .. } => ImConfigView::Feishu {
                app_id: app_id.clone(),
                secret_configured: true,
            },
        }
    }

    pub fn build(&self, state_dir: Option<&Path>) -> anyhow::Result<Provider> {
        match self {
            Self::Wechat { credentials } => Ok(Provider::Wechat(Arc::new(
                wechat::WechatClient::new(credentials.clone(), state_dir)?,
            ))),
            Self::Feishu { app_id, app_secret } => Ok(Provider::Feishu(Arc::new(
                feishu::FeishuClient::new(app_id.clone(), app_secret.clone())?,
            ))),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Wechat { credentials } => {
                credentials.validate().map_err(|error| error.to_string())
            }
            Self::Feishu { app_id, app_secret } => {
                if app_id.is_empty()
                    || app_id.len() > 256
                    || app_id.chars().any(char::is_whitespace)
                {
                    return Err("请填写有效的飞书 App ID".into());
                }
                if app_secret.is_empty()
                    || app_secret.len() > 4096
                    || app_secret.chars().any(char::is_control)
                {
                    return Err("请填写有效的飞书 App Secret".into());
                }
                Ok(())
            }
        }
    }
}

impl ImConfigUpdate {
    pub fn resolve(self, previous: &[ImConfig]) -> Result<ImConfig, String> {
        let config = match self {
            Self::Wechat {} => previous
                .iter()
                .find(|config| config.kind() == ImKind::Wechat)
                .cloned()
                .ok_or("请先扫码绑定微信 Bot")?,
            Self::Feishu { app_id, app_secret } => {
                let app_id = app_id.trim().to_owned();
                let secret = app_secret
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        previous.iter().find_map(|config| match config {
                            ImConfig::Feishu {
                                app_id: old_id,
                                app_secret,
                            } if *old_id == app_id => Some(app_secret.clone()),
                            _ => None,
                        })
                    })
                    .ok_or("请填写飞书 App Secret；更换 App ID 时需要同时填写新密钥")?;
                ImConfig::Feishu {
                    app_id,
                    app_secret: secret.trim().to_owned(),
                }
            }
        };
        config.validate()?;
        Ok(config)
    }
}

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
