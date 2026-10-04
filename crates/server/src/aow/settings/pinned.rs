use std::path::Path;

use serde::{Deserialize, Serialize};

use super::super::{AowError, AowManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::aow) struct PinnedDirectories {
    pub(in crate::aow) paths: Vec<String>,
    pub(in crate::aow) revision: u64,
}

#[derive(Debug, Default, Deserialize)]
pub(in crate::aow) struct UpdatePinnedDirectoriesRequest {
    #[serde(default)]
    pub(in crate::aow) add: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::aow) struct PinnedWorktrees {
    pub(in crate::aow) paths: Vec<String>,
    pub(in crate::aow) revision: u64,
}

#[derive(Debug, Default, Deserialize)]
pub(in crate::aow) struct UpdatePinnedWorktreesRequest {
    #[serde(default)]
    pub(in crate::aow) add: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) remove: Vec<String>,
    pub(in crate::aow) order: Option<Vec<String>>,
}

impl AowManager {
    pub(in crate::aow) fn pinned_worktrees(&self) -> Result<PinnedWorktrees, AowError> {
        let settings = self.lock_settings()?;
        Ok(PinnedWorktrees {
            paths: settings.pinned_worktrees.clone(),
            revision: settings.pinned_worktrees_revision,
        })
    }

    pub(in crate::aow) fn update_pinned_worktrees(
        &self,
        request: UpdatePinnedWorktreesRequest,
    ) -> Result<PinnedWorktrees, AowError> {
        for path in request
            .add
            .iter()
            .chain(&request.remove)
            .chain(request.order.iter().flatten())
        {
            if !Path::new(path).is_absolute() || path.contains('\0') {
                return Err(AowError::Invalid(
                    "pinned worktree must be an absolute path".to_owned(),
                ));
            }
        }
        let mut settings = self.lock_settings()?;
        let previous = settings.clone();
        settings
            .pinned_worktrees
            .retain(|path| !request.remove.contains(path));
        for path in request.add {
            if !settings.pinned_worktrees.contains(&path) {
                settings.pinned_worktrees.push(path);
            }
        }
        if let Some(order) = request.order {
            let mut ordered = Vec::new();
            for path in order {
                if settings.pinned_worktrees.contains(&path) && !ordered.contains(&path) {
                    ordered.push(path);
                }
            }
            for path in &settings.pinned_worktrees {
                if !ordered.contains(path) {
                    ordered.push(path.clone());
                }
            }
            settings.pinned_worktrees = ordered;
        }
        if settings.pinned_worktrees != previous.pinned_worktrees {
            settings.pinned_worktrees_revision += 1;
            if let Err(error) = self.persist_settings(&settings) {
                *settings = previous;
                return Err(error);
            }
        }
        Ok(PinnedWorktrees {
            paths: settings.pinned_worktrees.clone(),
            revision: settings.pinned_worktrees_revision,
        })
    }

    pub(in crate::aow) fn pinned_directories(&self) -> Result<PinnedDirectories, AowError> {
        let settings = self.lock_settings()?;
        Ok(PinnedDirectories {
            paths: settings.pinned_directories.clone(),
            revision: settings.pinned_directories_revision,
        })
    }

    pub(in crate::aow) async fn update_pinned_directories(
        &self,
        request: UpdatePinnedDirectoriesRequest,
    ) -> Result<PinnedDirectories, AowError> {
        let normalize = |path: String| -> Result<String, AowError> {
            if !Path::new(&path).is_absolute() || path.contains('\0') {
                return Err(AowError::Invalid(
                    "pinned directory must be an absolute path".to_owned(),
                ));
            }
            let path = path.trim_end_matches('/');
            Ok(if path.is_empty() { "/" } else { path }.to_owned())
        };
        let add = request
            .add
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()?;
        let remove = request
            .remove
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()?;
        for path in &add {
            if !tokio::fs::metadata(path).await?.is_dir() {
                return Err(AowError::Invalid(format!("not a directory: {path}")));
            }
        }
        // Removal also works for bookmarks whose directory was moved or deleted.
        let mut settings = self.lock_settings()?;
        let previous = settings.clone();
        settings
            .pinned_directories
            .retain(|path| !remove.contains(path));
        for path in add {
            if !settings.pinned_directories.contains(&path) {
                settings.pinned_directories.push(path);
            }
        }
        if settings.pinned_directories != previous.pinned_directories {
            settings.pinned_directories_revision += 1;
            if let Err(error) = self.persist_settings(&settings) {
                *settings = previous;
                return Err(error);
            }
        }
        Ok(PinnedDirectories {
            paths: settings.pinned_directories.clone(),
            revision: settings.pinned_directories_revision,
        })
    }
}
