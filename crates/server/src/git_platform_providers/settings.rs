use aow_config::ConfigStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

use super::{GitPlatformError, Result, repository_info::RepositoryInfoCache, validate_settings};

pub(super) const CONFIG: &str = "git-platform-providers.json";
pub(super) const DIRECTORY: &str = "git-platform-providers";
pub(super) const SCRIPT_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitPlatformProvider {
    pub id: String,
    pub name: String,
    pub hosts: Vec<String>,
    pub script: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub providers: Vec<GitPlatformProvider>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Document {
    pub(super) version: u32,
    pub(super) revision: u64,
    pub(super) providers: Vec<StoredProvider>,
    #[serde(default)]
    pub(super) bundled: BTreeMap<String, String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StoredProvider {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) hosts: Vec<String>,
}
impl From<&GitPlatformProvider> for StoredProvider {
    fn from(p: &GitPlatformProvider) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            hosts: p.hosts.clone(),
        }
    }
}
#[derive(Clone)]
pub struct GitPlatformProviderManager {
    inner: Arc<Mutex<Settings>>,
    pub(super) config: Option<ConfigStore>,
    bundled: Arc<Mutex<BTreeMap<String, String>>>,
    pub(super) repository_info: Arc<RepositoryInfoCache>,
}
impl Default for GitPlatformProviderManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Settings {
                revision: 0,
                providers: Vec::new(),
            })),
            config: None,
            bundled: Arc::default(),
            repository_info: Arc::default(),
        }
    }
}
impl GitPlatformProviderManager {
    pub fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        Self::persistent_with_defaults(state_dir, super::bundled::load()?)
    }
    pub(super) fn persistent_with_defaults(
        state_dir: &Path,
        defaults: Vec<GitPlatformProvider>,
    ) -> anyhow::Result<Self> {
        let config = ConfigStore::initialize(state_dir)?;
        let loaded = super::startup::load(&config, defaults)?;
        let mut settings = loaded.settings;
        let manager = Self {
            config: Some(config),
            bundled: Arc::new(Mutex::new(loaded.bundled)),
            ..Self::default()
        };
        if loaded.changed {
            settings.revision = settings
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("provider revision overflow"))?;
            manager.persist(&settings)?;
        }
        *manager.inner.lock().unwrap() = settings;
        Ok(manager)
    }
    pub(super) fn settings(&self) -> Result<Settings> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| {
                GitPlatformError::Command("GitPlatformProvider configuration lock failed".into())
            })?
            .clone())
    }
    pub(super) fn save(&self, mut next: Settings) -> Result<Settings> {
        validate_settings(&next)?;
        let mut current = self.inner.lock().map_err(|_| {
            GitPlatformError::Command("GitPlatformProvider configuration lock failed".into())
        })?;
        if current.revision != next.revision {
            return Err(GitPlatformError::Conflict);
        }
        next.revision = next.revision.checked_add(1).ok_or_else(|| {
            GitPlatformError::Invalid(
                aow_i18n::text("validation.configuration_revision_overflow").into(),
            )
        })?;
        self.persist(&next)?;
        if let Some(config) = &self.config {
            let removed: Vec<_> = current
                .providers
                .iter()
                .filter(|previous| !next.providers.iter().any(|p| p.id == previous.id))
                .map(|previous| previous.id.clone())
                .collect();
            if !removed.is_empty() {
                config
                    .update_directory(Path::new(DIRECTORY), |directory| {
                        for id in removed {
                            match std::fs::remove_file(directory.join(id)) {
                                Ok(()) => {}
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                                Err(error) => return Err(error.into()),
                            }
                        }
                        Ok(())
                    })
                    .map_err(|e| GitPlatformError::Command(e.to_string()))?;
            }
        }
        *current = next.clone();
        Ok(next)
    }
    pub(super) fn persist(&self, settings: &Settings) -> Result<()> {
        let Some(config) = &self.config else {
            return Ok(());
        };
        let mut providers = Vec::new();
        for p in &settings.providers {
            config
                .save(&Path::new(DIRECTORY).join(&p.id), p.script.as_bytes())
                .map_err(|e| GitPlatformError::Command(e.to_string()))?;
            providers.push(StoredProvider::from(p));
        }
        let doc = Document {
            version: 3,
            revision: settings.revision,
            providers,
            bundled: self
                .bundled
                .lock()
                .map_err(|_| {
                    GitPlatformError::Command("GitPlatformProvider defaults lock failed".into())
                })?
                .clone(),
        };
        config
            .save(Path::new(CONFIG), &serde_json::to_vec_pretty(&doc).unwrap())
            .map_err(|e| GitPlatformError::Command(e.to_string()))
    }
}
