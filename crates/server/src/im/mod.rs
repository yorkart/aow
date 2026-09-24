//! IM transport implementations. Notification dispatch only sees this interface.
use std::{future::Future, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::notifications::AutomationFailureNotification;
use crate::terminal::notifications::TaskStopNotification;

mod feishu;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImKind {
    Feishu,
}

// Credentials intentionally have no Debug implementation and are never returned by APIs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ImConfig {
    Feishu { app_id: String, app_secret: String },
}

#[derive(Serialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub(crate) enum ImConfigView {
    Feishu {
        app_id: String,
        secret_configured: bool,
    },
}

#[derive(Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ImConfigUpdate {
    Feishu {
        app_id: String,
        app_secret: Option<String>,
    },
}

impl ImConfig {
    pub(crate) fn kind(&self) -> ImKind {
        match self {
            Self::Feishu { .. } => ImKind::Feishu,
        }
    }

    pub(crate) fn view(&self) -> ImConfigView {
        match self {
            Self::Feishu { app_id, .. } => ImConfigView::Feishu {
                app_id: app_id.clone(),
                secret_configured: true,
            },
        }
    }

    pub(crate) fn build(&self) -> anyhow::Result<Provider> {
        match self {
            Self::Feishu { app_id, app_secret } => Ok(Provider::Feishu(Arc::new(
                feishu::FeishuClient::new(app_id.clone(), app_secret.clone())?,
            ))),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
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
    pub(crate) fn resolve(self, previous: &[ImConfig]) -> Result<ImConfig, String> {
        let config = match self {
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

pub(crate) trait ImProvider: Send + Sync {
    fn send_notification(
        &self,
        event: &TaskStopNotification,
        delivery_id: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn send_automation_failure(
        &self,
        event: &AutomationFailureNotification,
        delivery_id: &str,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[derive(Clone)]
pub(crate) enum Provider {
    Feishu(Arc<feishu::FeishuClient>),
}

impl ImProvider for Provider {
    async fn send_automation_failure(
        &self,
        event: &AutomationFailureNotification,
        delivery_id: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Feishu(client) => client.send_automation_failure(event, delivery_id).await,
        }
    }

    async fn send_notification(
        &self,
        event: &TaskStopNotification,
        delivery_id: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Feishu(client) => client.send_notification(event, delivery_id).await,
        }
    }
}
