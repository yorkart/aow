use crate::facade::SessionSnapshot;
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct ThreadStore {
    directory: Option<PathBuf>,
    pub(crate) threads: BTreeMap<String, SessionSnapshot>,
}

impl ThreadStore {
    pub(crate) fn new(directory: Option<PathBuf>) -> Result<Self> {
        let mut threads = BTreeMap::new();
        if let Some(directory) = &directory {
            std::fs::create_dir_all(directory)?;
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    let mut thread: SessionSnapshot =
                        serde_json::from_slice(&std::fs::read(&path)?).with_context(|| {
                            format!("Cannot load ACP history {}", path.display())
                        })?;
                    thread.status = "disconnected".into();
                    thread.permissions.clear();
                    super::AcpThread::restore_history(&mut thread);
                    threads.insert(thread.id.clone(), thread);
                }
            }
        }
        Ok(Self { directory, threads })
    }
    pub(crate) fn update(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut SessionSnapshot),
    ) -> Result<()> {
        let thread = self.threads.get_mut(id).context("ACP session not found")?;
        apply(thread);
        thread.revision += 1;
        thread.updated_at = chrono::Utc::now().to_rfc3339();
        if let Some(directory) = &self.directory {
            atomic_write(
                &directory.join(format!("{}.json", thread.id)),
                &serde_json::to_vec(thread)?,
            )?;
        }
        Ok(())
    }
    pub(crate) fn insert(&mut self, thread: SessionSnapshot) -> Result<()> {
        let id = thread.id.clone();
        self.threads.insert(id.clone(), thread);
        self.update(&id, |_| {})
    }
    // Unlike activity updates, importing metadata preserves its original timestamp.
    pub(crate) fn insert_metadata(&mut self, mut thread: SessionSnapshot) -> Result<()> {
        thread.revision += 1;
        if let Some(directory) = &self.directory {
            atomic_write(
                &directory.join(format!("{}.json", thread.id)),
                &serde_json::to_vec(&thread)?,
            )?;
        }
        self.threads.insert(thread.id.clone(), thread);
        Ok(())
    }
    pub(crate) fn remove(&mut self, id: &str) -> Result<()> {
        if self.threads.remove(id).is_some()
            && let Some(directory) = &self.directory
        {
            std::fs::remove_file(directory.join(format!("{id}.json")))?;
        }
        Ok(())
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let parent = path.parent().context("Missing parent directory")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
