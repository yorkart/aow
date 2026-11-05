//! Atomic Server configuration documents; schemas stay with their owning service.
use crate::error::ServiceError;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
pub(crate) const REGISTRY_VERSION: u32 = 1;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RegistryDocument<T> {
    pub(crate) version: u32,
    pub(crate) items: Vec<T>,
}

pub(crate) fn save_configuration<T: Serialize>(
    config: Option<&aow_config::ConfigStore>,
    path: &Path,
    document: &T,
) -> Result<(), ServiceError> {
    if let Some(config) = config {
        let relative = path
            .strip_prefix(config.directory())
            .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        let mut bytes = serde_json::to_vec_pretty(document)?;
        bytes.push(b'\n');
        config
            .save(relative, &bytes)
            .map_err(ServiceError::Configuration)?;
    }
    Ok(())
}
pub(crate) fn load_registry<T>(path: &Path) -> Result<Vec<T>, ServiceError>
where
    T: for<'de> Deserialize<'de>,
{
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: RegistryDocument<T> = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(ServiceError::Invalid(format!(
                    "unsupported AoW registry version {}",
                    document.version
                )));
            }
            Ok(document.items)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn atomic_save_document<T: Serialize>(
    path: &Path,
    document: &T,
) -> Result<(), ServiceError> {
    let parent = path.parent().ok_or_else(|| {
        ServiceError::Invalid(format!("registry path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("aow"),
        aow_id::new_id()
    ));
    let result = (|| {
        let bytes = serde_json::to_vec_pretty(document)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok::<_, ServiceError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
