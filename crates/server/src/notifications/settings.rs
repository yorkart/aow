use std::collections::HashSet;

use aow_im::{ImConfig, ImConfigUpdate, ImConfigView, ImKind};
use serde::{Deserialize, Serialize};

pub(super) const FILE_NAME: &str = "notification-settings.json";

pub(crate) struct AutomationFailureNotification {
    pub(crate) run: aow_automations::Run,
    pub(crate) run_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    Page,
    Feishu,
    Wechat,
}

impl Channel {
    pub(super) fn provider(self) -> Option<ImKind> {
        match self {
            Self::Page => None,
            Self::Feishu => Some(ImKind::Feishu),
            Self::Wechat => Some(ImKind::Wechat),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskCompletedSettings {
    pub(super) enabled: bool,
    pub(super) channels: Vec<Channel>,
}

impl Default for TaskCompletedSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            channels: vec![Channel::Page],
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImSettings {
    pub(super) providers: Vec<ImConfig>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NotificationSettings {
    #[serde(default)]
    pub(super) language: aow_i18n::Language,
    pub(super) agent_task_completed: TaskCompletedSettings,
    #[serde(default)]
    pub(super) public_base_url: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Document {
    pub(super) version: u32,
    pub(super) im: ImSettings,
    pub(super) notifications: NotificationSettings,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            im: ImSettings::default(),
            notifications: NotificationSettings::default(),
        }
    }
}

impl Document {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(
                aow_i18n::text("server.unsupported_notification_configuration_version").into(),
            );
        }
        normalize_public_base_url(&self.notifications.public_base_url)?;
        let mut providers = HashSet::new();
        for provider in &self.im.providers {
            provider.validate()?;
            if !providers.insert(provider.kind()) {
                return Err(aow_i18n::text("server.duplicate_im_configuration").into());
            }
        }
        let settings = &self.notifications.agent_task_completed;
        if settings.enabled && settings.channels.is_empty() {
            return Err(aow_i18n::text(
                "server.select_at_least_one_notification_method_when_enabling_agent",
            )
            .into());
        }
        let mut channels = HashSet::new();
        for channel in &settings.channels {
            if !channels.insert(channel) {
                return Err(aow_i18n::text("server.duplicate_notification_method").into());
            }
            if let Some(kind) = channel.provider()
                && !providers.contains(&kind)
            {
                let name = if kind == ImKind::Feishu {
                    aow_i18n::text("server.feishu")
                } else {
                    aow_i18n::text("server.wechat")
                };
                return Err(aow_i18n::message(
                    "server.configure_the_bot_first_deselect_push_before_removing_the",
                    &[("p0", name.to_string()), ("p1", name.to_string())],
                ));
            }
        }
        Ok(())
    }

    pub(super) fn view(&self) -> SettingsView {
        SettingsView {
            im: ImSettingsView {
                providers: self.im.providers.iter().map(ImConfig::view).collect(),
            },
            notifications: self.notifications.clone(),
        }
    }
}

#[derive(Serialize)]
struct ImSettingsView {
    providers: Vec<ImConfigView>,
}

#[derive(Serialize)]
pub(crate) struct SettingsView {
    im: ImSettingsView,
    pub(super) notifications: NotificationSettings,
}

#[derive(Deserialize)]
#[serde(tag = "section", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SettingsUpdate {
    ImProvider {
        config: ImConfigUpdate,
    },
    ImRemove {
        provider: ImKind,
    },
    #[serde(skip)]
    WechatBinding {
        credentials: aow_im::wechat::Credentials,
    },
    Im {
        providers: Vec<ImConfigUpdate>,
    },
    Notifications {
        language: Option<aow_i18n::Language>,
        agent_task_completed: TaskCompletedSettings,
        // Older clients only update delivery preferences and preserve the URL.
        public_base_url: Option<String>,
    },
}
pub(super) fn normalize_public_base_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let invalid =
        || aow_i18n::text("server.enter_a_valid_aow_http_s_deployment_url_optionally").to_owned();
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    if value.len() > 2048
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let base = crate::BasePath::parse(url.path()).map_err(|_| invalid())?;
    Ok(format!(
        "{}{}",
        url.origin().ascii_serialization(),
        base.as_str()
    ))
}
