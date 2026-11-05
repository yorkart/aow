use super::*;
use crate::runtime_connection::RuntimeConnection;
#[derive(Clone)]
pub(crate) struct SettingsService {
    pub(super) inner: Arc<SettingsState>,
    pub(super) node: RuntimeConnection,
}
pub(super) struct SettingsState {
    pub(super) config: Option<aow_config::ConfigStore>,
    pub(super) settings_path: Option<PathBuf>,
    pub(super) settings: Arc<Mutex<AowSettings>>,
    pub(super) operation: Arc<tokio::sync::Mutex<()>>,
}
impl SettingsService {
    pub(crate) fn new(
        config: Option<aow_config::ConfigStore>,
        notes_base: PathBuf,
        node: RuntimeConnection,
    ) -> Result<Self, ServiceError> {
        let settings_path = config
            .as_ref()
            .map(|c| c.directory().join(aow_config::environment::SETTINGS_FILE));
        let settings = if let Some(path) = &settings_path {
            load_settings(path, notes_base)?
        } else {
            AowSettings::with_notes_base(notes_base)
        };
        let settings = Arc::new(Mutex::new(settings));
        #[cfg(test)]
        node.bind_test_settings(settings.clone());
        Ok(Self {
            node,
            inner: Arc::new(SettingsState {
                config,
                settings_path,
                settings,
                operation: Arc::default(),
            }),
        })
    }
    pub(crate) async fn initialize_notes_base(&self, root: &Path) -> Result<(), ServiceError> {
        let _operation = self.inner.operation.lock().await;
        if self
            .inner
            .settings_path
            .as_ref()
            .is_some_and(|p| !p.exists())
        {
            let mut settings = self.lock_settings()?;
            let mut next = settings.clone();
            next.notes_base = root.join("data/notes").to_string_lossy().into_owned();
            self.persist_settings(&next)?;
            *settings = next;
        }
        Ok(())
    }
}

#[cfg(test)]
impl SettingsService {
    pub(crate) fn test_settings(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, AowSettings>, ServiceError> {
        self.lock_settings()
    }
    pub(crate) fn test_settings_path(&self) -> Option<&Path> {
        self.inner.settings_path.as_deref()
    }
}
