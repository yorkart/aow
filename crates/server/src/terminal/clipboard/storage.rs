use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use uuid::Uuid;

use super::super::*;
use super::*;

impl ClipboardStorage {
    pub(in crate::terminal) fn persistent(state_dir: &Path) -> Result<Self, TerminalError> {
        let state_dir = absolute_path(state_dir)?;
        let directory = state_dir.join(CLIPBOARD_DIRECTORY);
        create_private_directory(&directory, true)?;
        let storage = Self {
            directory: std::fs::canonicalize(directory)?,
            remove_on_drop: false,
            initialization_error: None,
        };
        storage.clean_expired()?;
        Ok(storage)
    }

    pub(in crate::terminal) fn temporary() -> Self {
        let parent = absolute_path(&std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
        for _ in 0..3 {
            let directory = parent.join(format!(
                "aow-clipboard-{}-{}",
                std::process::id(),
                Uuid::new_v4().as_simple()
            ));
            match create_private_directory(&directory, false) {
                Ok(()) => {
                    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
                    let storage = Self {
                        directory,
                        remove_on_drop: true,
                        initialization_error: None,
                    };
                    return match storage.clean_expired() {
                        Ok(_) => storage,
                        Err(error) => {
                            let mut storage = storage;
                            storage.initialization_error = Some(error.to_string());
                            storage
                        }
                    };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Self {
                        directory,
                        remove_on_drop: false,
                        initialization_error: Some(error.to_string()),
                    };
                }
            }
        }
        Self {
            directory: parent,
            remove_on_drop: false,
            initialization_error: Some(
                "could not allocate a unique temporary directory".to_owned(),
            ),
        }
    }

    pub(in crate::terminal) fn clean_expired(&self) -> Result<u64, TerminalError> {
        self.clean_expired_at(SystemTime::now())
    }

    pub(in crate::terminal) fn clean_expired_at(
        &self,
        now: SystemTime,
    ) -> Result<u64, TerminalError> {
        if let Some(error) = &self.initialization_error {
            return Err(TerminalError::ClipboardStorage(error.clone()));
        }
        let mut used_bytes = 0_u64;
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let expired = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= CLIPBOARD_IMAGE_TTL);
            if expired {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            } else {
                used_bytes = used_bytes.saturating_add(metadata.len());
            }
        }
        Ok(used_bytes)
    }
}

impl Drop for ClipboardStorage {
    fn drop(&mut self) {
        if self.remove_on_drop
            && let Err(error) = std::fs::remove_dir_all(&self.directory)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(
                %error,
                directory = %self.directory.display(),
                "failed to remove temporary clipboard image directory"
            );
        }
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, TerminalError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn create_private_directory(path: &Path, allow_existing: bool) -> std::io::Result<()> {
    let result = std::fs::DirBuilder::new()
        .recursive(allow_existing)
        .create(path);
    if let Err(error) = result
        && !(allow_existing && error.kind() == std::io::ErrorKind::AlreadyExists)
    {
        return Err(error);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) async fn open_private_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let file = options.open(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(file)
}
