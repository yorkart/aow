use std::path::{Path, PathBuf};

use aow_agents::environment;
use serde::{Deserialize, Serialize};

use super::super::{AowError, AowManager, REGISTRY_VERSION};
use super::model::{AowSettings, EditorSettings};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsDocument {
    version: u32,
    notes_base: String,
    #[serde(default)]
    node_addresses: Vec<String>,
    #[serde(default)]
    editor: EditorSettings,
    #[serde(default)]
    execution_path: Option<Vec<PathBuf>>,
    #[serde(default)]
    pinned_worktrees: Vec<String>,
    #[serde(default)]
    pinned_worktrees_revision: u64,
    #[serde(default)]
    pinned_directories: Vec<String>,
    #[serde(default)]
    pinned_directories_revision: u64,
}

#[derive(Debug, Default, Deserialize)]
pub(in crate::aow) struct UpdateSettingsRequest {
    pub(in crate::aow) notes_base: Option<String>,
    pub(in crate::aow) node_addresses: Option<Vec<String>>,
    pub(in crate::aow) execution_path: Option<Vec<PathBuf>>,
    pub(in crate::aow) editor: Option<EditorSettings>,
}

impl AowManager {
    pub(in crate::aow) fn persist_settings(&self, settings: &AowSettings) -> Result<(), AowError> {
        if let Some(path) = &self.inner.settings_path {
            self.save_configuration(
                path,
                &SettingsDocument {
                    version: REGISTRY_VERSION,
                    notes_base: settings.notes_base.clone(),
                    node_addresses: settings.node_addresses.clone(),
                    editor: settings.editor.clone(),
                    execution_path: settings.execution_path.clone(),
                    pinned_worktrees: settings.pinned_worktrees.clone(),
                    pinned_worktrees_revision: settings.pinned_worktrees_revision,
                    pinned_directories: settings.pinned_directories.clone(),
                    pinned_directories_revision: settings.pinned_directories_revision,
                },
            )?;
        }
        Ok(())
    }
}

pub(in crate::aow) fn default_notes_base() -> PathBuf {
    crate::PROCESS_HOME.join("aow")
}

pub(in crate::aow) async fn prepare_notes_base_directory(path: &Path) -> Result<PathBuf, AowError> {
    Ok(PathBuf::from(
        super::super::notes::prepare_notes_directory(path).await?,
    ))
}

pub(super) fn normalize_node_addresses(addresses: Vec<String>) -> Result<Vec<String>, AowError> {
    let mut normalized = Vec::new();
    for (index, address) in addresses.iter().enumerate() {
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        let url = reqwest::Url::parse(address).ok().filter(|url| {
            address
                .split_once("://")
                .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(url.scheme()))
                && matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        });
        let Some(url) = url else {
            return Err(AowError::Invalid(format!(
                "第 {} 个节点地址无效，请填写完整的 http:// 或 https:// 地址（不含用户名和密码）。",
                index + 1
            )));
        };
        let address = url.to_string();
        if !normalized.contains(&address) {
            normalized.push(address);
        }
    }
    Ok(normalized)
}

pub(in crate::aow) fn load_settings(
    path: &Path,
    default_notes_base: PathBuf,
) -> Result<AowSettings, AowError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: SettingsDocument = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(AowError::Invalid(format!(
                    "unsupported AoW settings version {}",
                    document.version
                )));
            }
            let notes_base = PathBuf::from(document.notes_base);
            if !notes_base.is_absolute() {
                return Err(AowError::Invalid(
                    "configured Notes root directory must be an absolute path".to_owned(),
                ));
            }
            Ok(AowSettings {
                notes_base: notes_base.to_string_lossy().into_owned(),
                node_addresses: document.node_addresses,
                editor: document.editor,
                execution_path: document
                    .execution_path
                    .map(environment::normalize_path)
                    .transpose()
                    .map_err(|error| AowError::Invalid(error.to_string()))?,
                pinned_worktrees: document.pinned_worktrees,
                pinned_worktrees_revision: document.pinned_worktrees_revision,
                pinned_directories: document.pinned_directories,
                pinned_directories_revision: document.pinned_directories_revision,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AowSettings::with_notes_base(default_notes_base))
        }
        Err(error) => Err(error.into()),
    }
}
