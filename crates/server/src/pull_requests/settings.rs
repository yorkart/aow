use aow_config::ConfigRepository;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use super::{
    PullRequestError, Result, managed_script_path, repository_info::RepositoryInfoCache,
    validate_settings,
};

pub(super) const CONFIG: &str = "review-providers.json";
pub(super) const SCRIPT_LIMIT: usize = 1024 * 1024;
pub(super) const GITHUB_SCRIPT: &str = include_str!("../review_adapters/github.py");
// Upgrade only exact previous bundled scripts, preserving customized adapters.
const PREVIOUS_GITHUB_SCRIPT_MD5: &[&str] = &[
    "d1e903f22605a41bf437bf77bc85a981",
    "28fea8f4f7673254cca56a6122bfd0a2",
    "135515472a8c25185cf5a81a35b89138",
    "04de57157c9355d6b2aea8f278930ecd",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub hosts: Vec<String>,
    pub script: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub providers: Vec<Provider>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Document {
    pub(super) version: u32,
    pub(super) revision: u64,
    pub(super) providers: Vec<StoredProvider>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct StoredProvider {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) enabled: bool,
    pub(super) hosts: Vec<String>,
    pub(super) script_file: String,
}
#[derive(Clone)]
pub struct ProviderManager {
    inner: Arc<Mutex<Settings>>,
    pub(super) config: Option<ConfigRepository>,
    pub(super) repository_info: Arc<RepositoryInfoCache>,
}
impl Default for ProviderManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Settings {
                revision: 0,
                providers: vec![Provider {
                    id: "github".into(),
                    name: "GitHub".into(),
                    enabled: true,
                    hosts: vec!["github.com".into()],
                    script: GITHUB_SCRIPT.into(),
                }],
            })),
            config: None,
            repository_info: Arc::default(),
        }
    }
}
impl ProviderManager {
    pub fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        let config = ConfigRepository::initialize(state_dir)?;
        let path = config.directory().join(CONFIG);
        let manager = Self {
            config: Some(config),
            ..Self::default()
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let doc: Document = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(
                    doc.version == 1,
                    "unsupported review provider config version"
                );
                let mut providers = Vec::new();
                for p in doc.providers {
                    anyhow::ensure!(
                        managed_script_path(&p.script_file),
                        "invalid managed script path"
                    );
                    let script = std::fs::read_to_string(
                        manager
                            .config
                            .as_ref()
                            .unwrap()
                            .directory()
                            .join(&p.script_file),
                    )?;
                    providers.push(Provider {
                        id: p.id,
                        name: p.name,
                        enabled: p.enabled,
                        hosts: p.hosts,
                        script,
                    });
                }
                let mut settings = Settings {
                    revision: doc.revision,
                    providers,
                };
                validate_settings(&settings)?;
                if upgrade_bundled_scripts(&mut settings, PREVIOUS_GITHUB_SCRIPT_MD5)? {
                    manager.persist(&settings)?;
                }
                *manager.inner.lock().unwrap() = settings;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                manager.persist(&manager.settings()?)?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(manager)
    }
    pub(super) fn settings(&self) -> Result<Settings> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| PullRequestError::Command("Provider configuration lock failed".into()))?
            .clone())
    }
    pub(super) fn save(&self, mut next: Settings) -> Result<Settings> {
        for provider in &mut next.providers {
            provider.name = provider.name.trim().into();
            provider.hosts = provider
                .hosts
                .iter()
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
        }
        validate_settings(&next)?;
        let mut current = self
            .inner
            .lock()
            .map_err(|_| PullRequestError::Command("Provider configuration lock failed".into()))?;
        if current.revision != next.revision {
            return Err(PullRequestError::Conflict);
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| PullRequestError::Invalid("configuration revision overflow".into()))?;
        self.persist(&next)?;
        *current = next.clone();
        Ok(next)
    }
    pub(super) fn persist(&self, settings: &Settings) -> Result<()> {
        let Some(config) = &self.config else {
            return Ok(());
        };
        let save = |path: &Path, bytes: &[u8]| {
            config
                .save(path, bytes)
                .map_err(|e| PullRequestError::Command(e.to_string()))
        };
        let mut providers = Vec::new();
        for p in &settings.providers {
            // Immutable versions: publishing the manifest last cannot expose a
            // half-written script, and concurrent calls keep their snapshot.
            let script_file = format!(
                "review-providers/{:x}.py",
                md5::compute(p.script.as_bytes())
            );
            save(Path::new(&script_file), p.script.as_bytes())?;
            providers.push(StoredProvider {
                id: p.id.clone(),
                name: p.name.clone(),
                enabled: p.enabled,
                hosts: p.hosts.clone(),
                script_file,
            });
        }
        let doc = Document {
            version: 1,
            revision: settings.revision,
            providers,
        };
        save(Path::new(CONFIG), &serde_json::to_vec_pretty(&doc).unwrap())
    }
}
pub(super) fn upgrade_bundled_scripts(
    settings: &mut Settings,
    previous_digests: &[&str],
) -> Result<bool> {
    let mut changed = false;
    let bundled = |script: &str| {
        previous_digests.contains(&format!("{:x}", md5::compute(script.as_bytes())).as_str())
    };
    for provider in &mut settings.providers {
        // A legacy copy capitalized the module's branding as AOW. Normalize
        // only that header; every other byte must match a known bundled script.
        let canonical = provider
            .script
            .strip_prefix("\"\"\"AOW review protocol")
            .map(|rest| format!("\"\"\"AoW review protocol{rest}"));
        if bundled(&provider.script) || canonical.as_deref().is_some_and(bundled) {
            provider.script = GITHUB_SCRIPT.into();
            changed = true;
        }
    }
    if changed {
        settings.revision = settings
            .revision
            .checked_add(1)
            .ok_or_else(|| PullRequestError::Invalid("configuration revision overflow".into()))?;
    }
    Ok(changed)
}
