use std::{collections::BTreeMap, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::*;

pub(super) const PROJECTS_FILE: &str = "aow-projects.json";
pub(super) const AGENTS_FILE: &str = "aow-agents.json";
pub(super) const REGISTRY_VERSION: u32 = 1;

#[derive(Default)]
pub(super) struct RegistryState {
    pub(super) projects: Vec<StoredProject>,
    pub(super) agents: Vec<StoredAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RegistryDocument<T> {
    pub(super) version: u32,
    pub(super) items: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredProject {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) registered_path: String,
    #[serde(default)]
    pub(super) common_git_dir: String,
    pub(super) notes_path: String,
    #[serde(default)]
    pub(super) notes_identity: Option<PathBuf>,
    #[serde(default)]
    pub(super) notes_custom: bool,
    #[serde(default)]
    pub(super) builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) avatar_url: Option<String>,
    #[serde(default)]
    pub(super) worktree_colors: BTreeMap<String, WorktreeColor>,
    #[serde(default)]
    pub(super) worktree_icons: BTreeMap<String, WorktreeIcon>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredAgent {
    pub(super) id: String,
    #[serde(default)]
    pub(super) agent_type: Option<AgentType>,
    pub(super) display_name: String,
    pub(super) command: String,
    #[serde(default)]
    pub(super) args: Vec<String>,
    #[serde(default)]
    pub(super) env: BTreeMap<String, String>,
}

impl AowManager {
    /// Registry metadata only; callers discover worktrees directly through Git.
    pub(super) fn persist_projects(&self, projects: &[StoredProject]) -> Result<(), AowError> {
        if let Some(path) = &self.inner.projects_path {
            self.save_configuration(
                path,
                &RegistryDocument {
                    version: REGISTRY_VERSION,
                    items: projects.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn persist_agents(&self, agents: &[StoredAgent]) -> Result<(), AowError> {
        if let Some(path) = &self.inner.agents_path {
            self.save_configuration(
                path,
                &RegistryDocument {
                    version: REGISTRY_VERSION,
                    items: agents.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn save_configuration<T: Serialize>(
        &self,
        path: &Path,
        document: &T,
    ) -> Result<(), AowError> {
        if let Some(config) = &self.inner.config {
            let relative = path
                .strip_prefix(config.directory())
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            let mut bytes = serde_json::to_vec_pretty(document)?;
            bytes.push(b'\n');
            config
                .save(relative, &bytes)
                .map_err(AowError::Configuration)?;
        }
        Ok(())
    }
}

pub(super) fn load_registry<T>(path: &Path) -> Result<Vec<T>, AowError>
where
    T: for<'de> Deserialize<'de>,
{
    match std::fs::read(path) {
        Ok(bytes) => {
            let document: RegistryDocument<T> = serde_json::from_slice(&bytes)?;
            if document.version != REGISTRY_VERSION {
                return Err(AowError::Invalid(format!(
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
) -> Result<(), AowError> {
    let parent = path.parent().ok_or_else(|| {
        AowError::Invalid(format!("registry path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("aow"),
        Uuid::new_v4().as_simple()
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
        Ok::<_, AowError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
