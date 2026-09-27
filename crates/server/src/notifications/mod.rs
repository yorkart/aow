//! Local notification settings and dispatch, independent of agent implementations.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header::CACHE_CONTROL},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::{
    AppState, HttpError,
    aow::{AowError, atomic_save_document},
    terminal::notifications::TaskStopNotification,
};

use aow_im::{ImConfig, ImConfigUpdate, ImConfigView, ImKind, ImProvider, Provider};

mod messages;

const FILE_NAME: &str = "notification-settings.json";
const QUEUE_CAPACITY: usize = 64;

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
    fn provider(self) -> Option<ImKind> {
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
    enabled: bool,
    channels: Vec<Channel>,
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
struct ImSettings {
    providers: Vec<ImConfig>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationSettings {
    agent_task_completed: TaskCompletedSettings,
    #[serde(default)]
    public_base_url: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    im: ImSettings,
    notifications: NotificationSettings,
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
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("不支持的通知配置版本".into());
        }
        normalize_public_base_url(&self.notifications.public_base_url)?;
        let mut providers = HashSet::new();
        for provider in &self.im.providers {
            provider.validate()?;
            if !providers.insert(provider.kind()) {
                return Err("IM 配置重复".into());
            }
        }
        let settings = &self.notifications.agent_task_completed;
        if settings.enabled && settings.channels.is_empty() {
            return Err("启用 Agent 任务完成通知后，至少选择一种通知方式".into());
        }
        let mut channels = HashSet::new();
        for channel in &settings.channels {
            if !channels.insert(channel) {
                return Err("通知方式重复".into());
            }
            if let Some(kind) = channel.provider()
                && !providers.contains(&kind)
            {
                let name = if kind == ImKind::Feishu {
                    "飞书"
                } else {
                    "微信"
                };
                return Err(format!(
                    "请先配置{name}机器人；移除机器人前请取消选择{name}推送"
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn view(&self) -> SettingsView {
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
    notifications: NotificationSettings,
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
        agent_task_completed: TaskCompletedSettings,
        // Older clients only update delivery preferences and preserve the URL.
        public_base_url: Option<String>,
    },
}

struct ConfiguredProvider {
    config: ImConfig,
    provider: Provider,
}

struct RuntimeState {
    document: Document,
    providers: Vec<ConfiguredProvider>,
    revision: u64,
}

struct Delivery {
    event: TaskStopNotification,
    revision: u64,
    id: String,
}

struct Inner {
    path: Option<PathBuf>,
    started: AtomicBool,
    wechat_login: aow_im::wechat::LoginManager,
    state: Mutex<RuntimeState>,
    sender: mpsc::Sender<Delivery>,
    receiver: Mutex<Option<mpsc::Receiver<Delivery>>>,
}

#[derive(Clone)]
pub(crate) struct NotificationManager {
    inner: Arc<Inner>,
}

impl NotificationManager {
    /// Automation preferences belong to the run snapshot, independently of
    /// interactive Agent completion preferences. False means no local bot.
    pub(crate) async fn send_automation_failure(
        &self,
        run: aow_automations::Run,
        channel: aow_automations::FailureNotification,
        delivery_id: &str,
    ) -> anyhow::Result<bool> {
        let Some((provider, event)) = self.automation_delivery(run, channel) else {
            return Ok(false);
        };
        provider
            .send(&messages::automation_failure(&event), delivery_id)
            .await?;
        Ok(true)
    }

    fn automation_delivery(
        &self,
        run: aow_automations::Run,
        channel: aow_automations::FailureNotification,
    ) -> Option<(Provider, AutomationFailureNotification)> {
        let (provider, base_url) = {
            let state = self.inner.state.lock().unwrap();
            (
                state
                    .providers
                    .iter()
                    .find(|provider| {
                        provider.config.kind()
                            == match channel {
                                aow_automations::FailureNotification::Feishu => ImKind::Feishu,
                                aow_automations::FailureNotification::Wechat => ImKind::Wechat,
                            }
                    })
                    .map(|provider| provider.provider.clone()),
                state.document.notifications.public_base_url.clone(),
            )
        };
        let provider = provider?;
        let run_url = reqwest::Url::parse(&base_url).ok().and_then(|mut url| {
            url.path_segments_mut().ok()?.pop_if_empty().extend([
                "aow",
                "tabs",
                "automation",
                &run.task_id,
                "runs",
                &run.id,
            ]);
            Some(url.into())
        });
        Some((provider, AutomationFailureNotification { run, run_url }))
    }

    pub(crate) fn in_memory() -> Self {
        Self::new(None, Document::default()).expect("empty configuration is valid")
    }

    pub(crate) fn persistent(state_dir: &Path) -> Result<Self, AowError> {
        let path = state_dir.join(FILE_NAME);
        let document = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Document::default(),
            Err(error) => return Err(error.into()),
        };
        Self::new(Some(path), document)
    }

    fn new(path: Option<PathBuf>, document: Document) -> Result<Self, AowError> {
        document.validate().map_err(AowError::Invalid)?;
        let providers = build_providers(
            &document.im.providers,
            &[],
            path.as_deref().and_then(Path::parent),
        )?;
        let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                started: AtomicBool::new(false),
                wechat_login: aow_im::wechat::LoginManager::new()
                    .map_err(AowError::Configuration)?,
                state: Mutex::new(RuntimeState {
                    document,
                    providers,
                    revision: 0,
                }),
                sender,
                receiver: Mutex::new(Some(receiver)),
            }),
        })
    }

    pub(crate) fn view(&self) -> SettingsView {
        self.inner.state.lock().unwrap().document.view()
    }

    pub(crate) fn update(&self, update: SettingsUpdate) -> Result<SettingsView, AowError> {
        let mut state = self.inner.state.lock().unwrap();
        let mut next = state.document.clone();
        match update {
            SettingsUpdate::WechatBinding { credentials } => {
                next.im
                    .providers
                    .retain(|config| config.kind() != ImKind::Wechat);
                next.im.providers.push(ImConfig::Wechat { credentials });
            }
            SettingsUpdate::ImProvider { config } => {
                let config = config
                    .resolve(&state.document.im.providers)
                    .map_err(AowError::Invalid)?;
                next.im.providers.retain(|old| old.kind() != config.kind());
                next.im.providers.push(config);
            }
            SettingsUpdate::ImRemove { provider } => {
                next.im.providers.retain(|config| config.kind() != provider);
            }
            SettingsUpdate::Im { providers } => {
                next.im.providers = providers
                    .into_iter()
                    .map(|update| update.resolve(&state.document.im.providers))
                    .collect::<Result<_, _>>()
                    .map_err(AowError::Invalid)?;
                // Legacy clients replace the Feishu list. They cannot disconnect
                // a QR binding that was added after they loaded their settings.
                if !next
                    .im
                    .providers
                    .iter()
                    .any(|config| config.kind() == ImKind::Wechat)
                {
                    next.im.providers.extend(
                        state
                            .document
                            .im
                            .providers
                            .iter()
                            .filter(|config| config.kind() == ImKind::Wechat)
                            .cloned(),
                    );
                }
            }
            SettingsUpdate::Notifications {
                agent_task_completed,
                public_base_url,
            } => {
                next.notifications.agent_task_completed = agent_task_completed;
                if let Some(url) = public_base_url {
                    next.notifications.public_base_url =
                        normalize_public_base_url(&url).map_err(AowError::Invalid)?;
                }
            }
        }
        next.validate().map_err(AowError::Invalid)?;
        let providers = build_providers(
            &next.im.providers,
            &state.providers,
            self.inner.path.as_deref().and_then(Path::parent),
        )?;
        if let Some(path) = &self.inner.path {
            atomic_save_document(path, &next)?;
        }
        for previous in &state.providers {
            if !next.im.providers.contains(&previous.config) {
                previous.provider.retire();
            }
        }
        if self.inner.started.load(Ordering::Relaxed) {
            for configured in &providers {
                configured.provider.start();
            }
        }
        state.document = next;
        state.providers = providers;
        state.revision += 1;
        Ok(state.document.view())
    }

    pub(crate) fn wechat_login(&self) -> &aow_im::wechat::LoginManager {
        &self.inner.wechat_login
    }

    pub(crate) fn wechat_credentials(&self) -> Option<aow_im::wechat::Credentials> {
        self.inner
            .state
            .lock()
            .unwrap()
            .document
            .im
            .providers
            .iter()
            .find_map(|config| match config {
                ImConfig::Wechat { credentials } => Some(credentials.clone()),
                _ => None,
            })
    }

    pub(crate) fn wechat(&self) -> Option<Arc<aow_im::wechat::WechatClient>> {
        self.inner
            .state
            .lock()
            .unwrap()
            .providers
            .iter()
            .find_map(|config| match &config.provider {
                Provider::Wechat(client) => Some(client.clone()),
                _ => None,
            })
    }

    pub(crate) fn start(&self) {
        self.inner.started.store(true, Ordering::Relaxed);
        for configured in &self.inner.state.lock().unwrap().providers {
            configured.provider.start();
        }
        let Some(mut receiver) = self.inner.receiver.lock().unwrap().take() else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            while let Some(delivery) = receiver.recv().await {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let providers: Vec<_> = {
                    let state = inner.state.lock().unwrap();
                    let settings = &state.document.notifications.agent_task_completed;
                    // Settings changes invalidate queued deliveries, including changed credentials.
                    if state.revision != delivery.revision || !settings.enabled {
                        continue;
                    }
                    settings
                        .channels
                        .iter()
                        .filter_map(|channel| channel.provider())
                        .filter_map(|kind| {
                            state
                                .providers
                                .iter()
                                .find(|provider| provider.config.kind() == kind)
                        })
                        .map(|provider| (provider.config.kind(), provider.provider.clone()))
                        .collect()
                };
                drop(inner);
                for (kind, provider) in providers {
                    if let Err(error) = provider
                        .send(&messages::task_completed(&delivery.event), &delivery.id)
                        .await
                    {
                        tracing::warn!(provider = ?kind, session_id = %delivery.event.session_id, %error, "agent notification delivery failed");
                    }
                }
            }
        });
    }

    pub(crate) fn dispatch(
        &self,
        mut event: TaskStopNotification,
        page: &broadcast::Sender<TaskStopNotification>,
    ) {
        let state = self.inner.state.lock().unwrap();
        let settings = &state.document.notifications.agent_task_completed;
        if !settings.enabled {
            return;
        }
        if let Ok(base) = reqwest::Url::parse(&state.document.notifications.public_base_url) {
            for source in &mut event.sources {
                let mut url = base.clone();
                if let Ok(mut segments) = url.path_segments_mut() {
                    segments
                        .pop_if_empty()
                        .extend(["aow", "tabs", "terminal", &source.tab_id]);
                }
                source.tab_url = Some(url.into());
            }
        }
        if settings.channels.contains(&Channel::Page) {
            let _ = page.send(event.clone());
        }
        if settings
            .channels
            .iter()
            .any(|channel| channel.provider().is_some())
        {
            let delivery = Delivery {
                event,
                revision: state.revision,
                id: Uuid::new_v4().to_string(),
            };
            if self.inner.sender.try_send(delivery).is_err() {
                tracing::warn!("IM notification queue full or closed; delivery dropped");
            }
        }
    }
}

fn normalize_public_base_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let invalid =
        || "请填写有效的 AoW HTTP(S) 部署地址，可含 Base Path，不含账号、查询参数或片段".to_owned();
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

fn build_providers(
    configs: &[ImConfig],
    previous: &[ConfiguredProvider],
    state_dir: Option<&Path>,
) -> Result<Vec<ConfiguredProvider>, AowError> {
    configs
        .iter()
        .map(|config| {
            let provider = match previous.iter().find(|old| old.config == *config) {
                Some(old) => old.provider.clone(),
                None => config.build(state_dir).map_err(AowError::Configuration)?,
            };
            Ok(ConfiguredProvider {
                config: config.clone(),
                provider,
            })
        })
        .collect()
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/aow/notification-settings",
        get(get_settings).put(update_settings),
    )
}

async fn get_settings(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    (
        [(CACHE_CONTROL, "no-store")],
        Json(state.aow.notifications().view()),
    )
}

async fn update_settings(
    State(state): State<AppState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<impl axum::response::IntoResponse, HttpError> {
    let manager = state.aow.notifications().clone();
    let settings = tokio::task::spawn_blocking(move || manager.update(update))
        .await
        .map_err(|_| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "notification_settings_failed",
                "无法保存通知配置",
                None,
            )
        })?
        .map_err(crate::aow::aow_http_error)?;
    Ok(([(CACHE_CONTROL, "no-store")], Json(settings)))
}

#[cfg(test)]
mod tests;
