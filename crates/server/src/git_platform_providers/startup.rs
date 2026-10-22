//! Reconcile persisted enable flags and release resources with files in the active config.
use super::{
    GitPlatformProvider, Settings, defaults,
    settings::{CONFIG, DIRECTORY, Document, StoredProvider},
    validate_settings,
};
use anyhow::ensure;
use aow_config::ConfigRepository;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

pub(super) struct Loaded {
    pub settings: Settings,
    pub bundled: BTreeMap<String, String>,
    pub retained: Vec<StoredProvider>,
    pub changed: bool,
}

pub(super) fn load(
    config: &ConfigRepository,
    defaults: Vec<GitPlatformProvider>,
) -> anyhow::Result<Loaded> {
    let mut changed = false;
    let mut document = match std::fs::read(config.directory().join(CONFIG)) {
        Ok(bytes) => {
            let document: Document = serde_json::from_slice(&bytes)?;
            ensure!(
                document.version == 3,
                "unsupported Git platform provider config version"
            );
            for entry in &document.providers {
                ensure!(
                    super::validation::valid_filename(&entry.id),
                    "invalid provider filename"
                );
            }
            document
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            changed = true;
            Document {
                version: 3,
                revision: 0,
                providers: vec![],
                bundled: BTreeMap::new(),
            }
        }
        Err(error) => return Err(error.into()),
    };
    let directory = config.directory().join(DIRECTORY);
    if let Ok(metadata) = std::fs::symlink_metadata(&directory) {
        ensure!(
            metadata.is_dir(),
            "provider directory must be a real directory"
        );
    }
    for provider in defaults {
        let file = directory.join(&provider.id);
        let previous = document.bundled.get(&provider.id);
        let next = digest(&provider.script);
        let install = if case_collision(&directory, &provider.id)? {
            false
        } else {
            match std::fs::symlink_metadata(&file) {
                Ok(_) => defaults::read_script(&file).ok().is_some_and(|script| {
                    previous.is_some_and(|previous| *previous == digest(&script))
                        && script != provider.script
                }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => previous.is_none(),
                Err(error) => return Err(error.into()),
            }
        };
        if install {
            config.save(
                &Path::new(DIRECTORY).join(&provider.id),
                provider.script.as_bytes(),
            )?;
            changed = true;
        }
        if document.bundled.insert(provider.id, next.clone()).as_ref() != Some(&next) {
            changed = true;
        }
    }
    // This is the sole runtime loading source, including newly installed defaults.
    let mut providers = defaults::load(&directory)?;
    providers.sort_by_key(|p| {
        document
            .providers
            .iter()
            .position(|entry| entry.id == p.id)
            .unwrap_or(usize::MAX)
    });
    let mut hosts = HashSet::new();
    for provider in &mut providers {
        provider.enabled = document
            .providers
            .iter()
            .find(|entry| entry.id == provider.id)
            .is_none_or(|entry| entry.enabled);
        if provider.enabled && provider.hosts.iter().any(|host| hosts.contains(host)) {
            tracing::warn!(provider = %provider.id, "Disabling Git platform provider with conflicting remote domains");
            provider.enabled = false;
        }
        if provider.enabled {
            hosts.extend(provider.hosts.iter().cloned());
        }
    }
    // Preserve enable flags for temporarily invalid scripts so repairing a disabled
    // file cannot accidentally enable it. These records never cause scripts to load.
    let retained: Vec<_> = document
        .providers
        .iter()
        .filter(|entry| {
            !providers.iter().any(|p| p.id == entry.id)
                && std::fs::symlink_metadata(directory.join(&entry.id)).is_ok()
        })
        .cloned()
        .collect();
    let entries: Vec<_> = providers
        .iter()
        .map(StoredProvider::from)
        .chain(retained.iter().cloned())
        .collect();
    changed |= entries != document.providers;
    let settings = Settings {
        revision: document.revision,
        providers,
    };
    validate_settings(&settings)?;
    Ok(Loaded {
        settings,
        bundled: document.bundled,
        retained,
        changed,
    })
}

fn digest(script: &str) -> String {
    format!("{:x}", md5::compute(script.as_bytes()))
}
fn case_collision(directory: &Path, id: &str) -> anyhow::Result<bool> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        if entry?
            .file_name()
            .to_str()
            .is_some_and(|name| name != id && name.eq_ignore_ascii_case(id))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
