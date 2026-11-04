use std::{path::Path, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{feishu, provider::Provider, wechat};

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
                    return Err(aow_i18n::text("im.enter_a_valid_feishu_app_id").into());
                }
                if app_secret.is_empty()
                    || app_secret.len() > 4096
                    || app_secret.chars().any(char::is_control)
                {
                    return Err(aow_i18n::text("im.enter_a_valid_feishu_app_secret").into());
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
                .ok_or(aow_i18n::text("im.scan_to_link_a_wechat_bot_first"))?,
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
                    .ok_or(aow_i18n::text(
                        "im.enter_the_feishu_app_secret_changing_the_app_id",
                    ))?;
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
