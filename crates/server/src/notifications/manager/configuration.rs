use super::*;
use std::{path::Path, sync::atomic::Ordering};

use crate::aow::{AowError, atomic_save_document};
use crate::notifications::{
    SettingsUpdate, SettingsView,
    settings::{FILE_NAME, normalize_public_base_url},
};
use aow_im::{ImConfig, ImKind, Provider};

impl NotificationManager {
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
                security: security::Dispatcher::default(),
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
