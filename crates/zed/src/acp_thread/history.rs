//! AoW live snapshot cache, backed by Zed's metadata-store operation boundary.
use crate::{
    agent_ui::thread_metadata_store::ThreadMetadataStore,
    facade::{SessionInfo, SessionSnapshot},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct ThreadStore {
    pub(crate) metadata: ThreadMetadataStore,
    // Only opened/active conversations are hydrated. Listing history never decodes their bodies.
    pub(crate) threads: BTreeMap<String, SessionSnapshot>,
}
impl ThreadStore {
    pub(crate) fn new(state_directory: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            metadata: ThreadMetadataStore::new(state_directory.as_deref())?,
            threads: BTreeMap::new(),
        })
    }
    pub(crate) fn sessions(&self, cwd: Option<&str>) -> Vec<SessionInfo> {
        let mut sessions: Vec<_> = self
            .metadata
            .entries()
            .filter(|row| cwd.is_none_or(|cwd| row.cwd() == cwd))
            .map(|row| {
                self.threads
                    .get(&row.thread_id)
                    .map(SessionInfo::from)
                    .unwrap_or_else(|| SessionInfo {
                        id: row.thread_id.clone(),
                        remote_id: row.session_id.clone().unwrap_or_default(),
                        agent_id: row.agent_id.clone(),
                        cwd: row.cwd().into(),
                        title: row.display_title().into(),
                        status: "disconnected".into(),
                        updated_at: row.updated_at.clone(),
                    })
            })
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions
    }
    fn load_thread(&mut self, id: &str) -> Result<()> {
        if self.threads.contains_key(id) {
            return Ok(());
        }
        let metadata = self.metadata.entry(id).context("ACP session not found")?;
        let mut thread = if let Some(thread) = self.metadata.load_thread(id)? {
            ensure!(
                thread.agent_id == metadata.agent_id
                    && Some(&thread.remote_id) == metadata.session_id.as_ref()
                    && thread.cwd == metadata.cwd(),
                "ACP snapshot identity does not match its metadata"
            );
            thread
        } else {
            let mut thread = super::AcpThread::new_snapshot(
                id.into(),
                metadata.session_id.clone().unwrap_or_default(),
                metadata.agent_id.clone(),
                metadata.cwd().into(),
            );
            thread.title = metadata.display_title().into();
            thread.updated_at = metadata.updated_at.clone();
            thread.needs_load = true;
            thread
        };
        thread.status = "disconnected".into();
        thread.permissions.clear();
        super::AcpThread::restore_history(&mut thread);
        self.threads.insert(id.into(), thread);
        Ok(())
    }
    pub(crate) fn snapshot(&mut self, id: &str) -> Result<SessionSnapshot> {
        self.load_thread(id)?;
        Ok(self.threads[id].clone())
    }
    pub(crate) fn update(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut SessionSnapshot),
    ) -> Result<()> {
        self.load_thread(id)?;
        let thread = self.threads.get_mut(id).context("ACP session not found")?;
        let working = thread.status == "working";
        let permissions: Vec<_> = thread
            .permissions
            .iter()
            .map(|permission| permission.id.clone())
            .collect();
        apply(thread);
        thread.revision += 1;
        thread.updated_at = chrono::Utc::now().to_rfc3339();
        let flush = !working
            || thread.status != "working"
            || permissions
                != thread
                    .permissions
                    .iter()
                    .map(|permission| permission.id.clone())
                    .collect::<Vec<_>>();
        self.metadata.save(thread);
        // Streamed text/tool updates coalesce in the background. Lifecycle and permission
        // boundaries are durable before returning to the agent or HTTP caller.
        if flush {
            self.metadata.flush()?;
        }
        Ok(())
    }
    pub(crate) fn insert(&mut self, mut thread: SessionSnapshot) -> Result<()> {
        thread.updated_at = chrono::Utc::now().to_rfc3339();
        self.insert_metadata(thread)
    }
    // Preserve provider timestamps when registering imported metadata.
    pub(crate) fn insert_metadata(&mut self, thread: SessionSnapshot) -> Result<()> {
        self.insert_metadata_all(vec![thread])
    }
    pub(crate) fn insert_metadata_all(&mut self, mut threads: Vec<SessionSnapshot>) -> Result<()> {
        for thread in &mut threads {
            thread.revision += 1;
        }
        self.metadata.save_all(&threads);
        self.threads.extend(
            threads
                .into_iter()
                .map(|thread| (thread.id.clone(), thread)),
        );
        self.metadata.flush()
    }
    pub(crate) fn remove(&mut self, id: &str) -> Result<()> {
        self.threads.remove(id);
        self.metadata.delete(id);
        self.metadata.flush()
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
