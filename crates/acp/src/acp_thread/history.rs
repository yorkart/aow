//! AoW live snapshot cache, backed by the upstream implementation's metadata-store operation boundary.
use crate::{
    agent_ui::thread_metadata_store::ThreadMetadataStore,
    facade::{SessionInfo, SessionSnapshot},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(crate) struct ThreadStore {
    pub(crate) events: crate::facade::events::SyncEvents,
    pub(crate) metadata: ThreadMetadataStore,
    // Only opened/active conversations are hydrated. Listing history never decodes their bodies.
    pub(crate) threads: BTreeMap<String, Arc<Mutex<SessionSnapshot>>>,
    // Fork RPCs reserve prompt admission without changing the source transcript.
    pub(crate) forking: HashSet<String>,
    metadata_changed: Box<dyn Fn() + Send + Sync>,
}
impl ThreadStore {
    pub(crate) fn new(
        state_directory: Option<PathBuf>,
        metadata_changed: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Ok(Self {
            events: Default::default(),
            metadata: ThreadMetadataStore::new(state_directory.as_deref())?,
            threads: BTreeMap::new(),
            forking: HashSet::new(),
            metadata_changed: Box::new(metadata_changed),
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
                    .map(|thread| SessionInfo::from(&*thread.lock().unwrap()))
                    .unwrap_or_else(|| SessionInfo {
                        thread_id: row.thread_id.clone(),
                        parent: row.parent.clone(),
                        source: row.source,
                        created_at: row
                            .created_at
                            .clone()
                            .unwrap_or_else(|| row.updated_at.clone()),
                        session_id: row.session_id.clone(),
                        agent_id: row.agent_id.clone(),
                        cwd: row.cwd().into(),
                        title: row.display_title().into(),
                        status: "disconnected".into(),
                        updated_at: row.updated_at.clone(),
                        interacted_at: row.interacted_at.clone(),
                    })
            })
            .collect();
        sessions.sort_by(|a, b| {
            b.interacted_at
                .as_ref()
                .unwrap_or(&b.updated_at)
                .cmp(a.interacted_at.as_ref().unwrap_or(&a.updated_at))
        });
        sessions
    }
    pub(crate) fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        let row = self
            .metadata
            .entry(thread_id)
            .context("ACP session not found")?;
        Ok(self
            .threads
            .get(thread_id)
            .map(|thread| SessionInfo::from(&*thread.lock().unwrap()))
            .unwrap_or_else(|| SessionInfo {
                thread_id: row.thread_id.clone(),
                parent: row.parent.clone(),
                source: row.source,
                created_at: row
                    .created_at
                    .clone()
                    .unwrap_or_else(|| row.updated_at.clone()),
                session_id: row.session_id.clone(),
                agent_id: row.agent_id.clone(),
                cwd: row.cwd().into(),
                title: row.display_title().into(),
                status: "disconnected".into(),
                updated_at: row.updated_at.clone(),
                interacted_at: row.interacted_at.clone(),
            }))
    }
    fn load_thread(&mut self, id: &str) -> Result<()> {
        if self.threads.contains_key(id) {
            return Ok(());
        }
        let metadata = self.metadata.entry(id).context("ACP session not found")?;
        let mut thread = if let Some(thread) = self.metadata.load_thread(id)? {
            ensure!(
                thread.agent_id == metadata.agent_id
                    && thread.session_id == metadata.session_id
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
            thread.parent = metadata.parent.clone();
            thread.title = metadata.display_title().into();
            thread.updated_at = metadata.updated_at.clone();
            thread.needs_load = true;
            thread
        };
        thread.source = metadata.source;
        // Freeze legacy/imported recency before recovery updates updated_at.
        thread.interacted_at = Some(
            metadata
                .interacted_at
                .clone()
                .unwrap_or_else(|| metadata.updated_at.clone()),
        );
        thread.created_at = metadata
            .created_at
            .clone()
            .unwrap_or_else(|| metadata.updated_at.clone());
        thread.status = "disconnected".into();
        super::steering::disconnected(&mut thread);
        thread.recovery = None;
        super::turns::finish(&mut thread);
        thread.permissions.clear();
        super::terminals::normalize_snapshot(&mut thread);
        thread.sync_generation = uuid::Uuid::new_v4().to_string();
        thread.changes.reset(thread.revision);
        self.threads.insert(id.into(), Arc::new(Mutex::new(thread)));
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn snapshot(&mut self, id: &str) -> Result<SessionSnapshot> {
        Ok(self.thread(id)?.lock().unwrap().clone())
    }
    pub(crate) fn thread(&mut self, id: &str) -> Result<Arc<Mutex<SessionSnapshot>>> {
        self.load_thread(id)?;
        Ok(self.threads[id].clone())
    }
    pub(crate) fn update(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut SessionSnapshot),
    ) -> Result<()> {
        self.update_inner(id, apply, false)
    }
    pub(crate) fn session_update(&mut self, id: &str, update: serde_json::Value) -> Result<()> {
        let incremental = super::steering::activity(&update).is_none();
        self.update_inner(
            id,
            |thread| super::AcpThread::handle_session_update(thread, update),
            incremental,
        )
    }
    fn update_inner(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut SessionSnapshot),
        incremental: bool,
    ) -> Result<()> {
        self.load_thread(id)?;
        let source = self.threads.get(id).context("ACP session not found")?;
        let mut thread = source.lock().unwrap();
        let working = thread.status == "working";
        let title = thread.title.clone();
        let permissions: Vec<_> = thread
            .permissions
            .iter()
            .map(|permission| permission.id.clone())
            .collect();
        apply(&mut thread);
        super::turns::finish(&mut thread);
        thread.revision += 1;
        thread.updated_at = chrono::Utc::now().to_rfc3339();
        let revision = thread.revision;
        if incremental && working {
            thread.changes.commit(revision);
        } else {
            thread.changes.reset(revision);
        }
        let flush = !working
            || thread.status != "working"
            || permissions
                != thread
                    .permissions
                    .iter()
                    .map(|permission| permission.id.clone())
                    .collect::<Vec<_>>();
        let head = crate::facade::SessionVersion::from(&*thread);
        let title_changed = title != thread.title;
        let urgent = flush || title_changed;
        drop(thread);
        self.metadata.save(source);
        // Streamed text/tool updates coalesce in the background. Lifecycle and permission
        // boundaries are durable before returning to the agent or HTTP caller.
        if flush {
            self.metadata.flush()?;
        }
        self.publish(head, urgent);
        if title_changed {
            (self.metadata_changed)();
        }
        Ok(())
    }
    fn publish(&self, head: crate::facade::SessionVersion, urgent: bool) {
        use crate::facade::SyncResource;
        self.events.publish(
            SyncResource::Sessions {
                cwd: head.info.cwd.clone(),
            },
            None,
            false,
            false,
        );
        self.events.publish(
            SyncResource::Session {
                thread_id: head.info.thread_id.clone(),
            },
            Some(head),
            false,
            urgent,
        );
    }
    pub(crate) fn insert(&mut self, mut thread: SessionSnapshot) -> Result<()> {
        thread.updated_at = chrono::Utc::now().to_rfc3339();
        self.insert_metadata(thread)
    }
    /// Keep the installed model and the writer's retry queue on the same snapshot.
    pub(crate) fn replace(&mut self, thread: SessionSnapshot) -> Result<()> {
        let id = thread.thread_id.clone();
        let previous = self.thread(&id)?;
        if let Err(error) = self.insert(thread) {
            self.threads.insert(id, previous.clone());
            // A failed transaction retains its queued write. Replace that write as
            // well, so a later flush cannot persist the rejected replay.
            self.metadata.save(&previous);
            return Err(error);
        }
        Ok(())
    }
    // Preserve provider timestamps when registering imported metadata.
    pub(crate) fn insert_metadata(&mut self, thread: SessionSnapshot) -> Result<()> {
        self.insert_metadata_all(vec![thread])
    }
    pub(crate) fn insert_metadata_all(&mut self, mut threads: Vec<SessionSnapshot>) -> Result<()> {
        for thread in &mut threads {
            thread.revision += 1;
            thread.changes.reset(thread.revision);
        }
        let heads: Vec<_> = threads
            .iter()
            .map(crate::facade::SessionVersion::from)
            .collect();
        for thread in threads {
            let id = thread.thread_id.clone();
            let thread = Arc::new(Mutex::new(thread));
            self.metadata.save(&thread);
            self.threads.insert(id, thread);
        }
        self.metadata.flush()?;
        for head in heads {
            self.publish(head, true);
        }
        (self.metadata_changed)();
        Ok(())
    }
    pub(crate) fn remove(&mut self, id: &str) -> Result<()> {
        let info = self.session_info(id)?;
        self.threads.remove(id);
        self.metadata.delete(id);
        self.metadata.flush()?;
        use crate::facade::SyncResource;
        self.events.publish(
            SyncResource::Session {
                thread_id: id.into(),
            },
            None,
            true,
            true,
        );
        self.events.publish(
            SyncResource::Connection {
                thread_id: id.into(),
            },
            None,
            true,
            true,
        );
        self.events
            .changed(SyncResource::Sessions { cwd: info.cwd });
        (self.metadata_changed)();
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

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::facade::SyncResource;

    #[test]
    fn committed_versions_cover_creation_streaming_permissions_titles_and_deletion() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let invalidations = Arc::new(AtomicUsize::new(0));
        let observed = invalidations.clone();
        let mut store = ThreadStore::new(None, move || {
            observed.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let mut events = store.events.subscribe();
        store
            .insert(super::super::AcpThread::new_snapshot(
                "thread".into(),
                "remote".into(),
                "test".into(),
                "/repo".into(),
            ))
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap().resource,
            SyncResource::Sessions { .. }
        ));
        let created = events.try_recv().unwrap();
        assert_eq!(
            created.session.unwrap().revision,
            store.snapshot("thread").unwrap().revision
        );
        assert_eq!(invalidations.load(Ordering::SeqCst), 1);
        store
            .update("thread", |thread| thread.status = "working".into())
            .unwrap();
        events.try_recv().unwrap();
        assert!(events.try_recv().unwrap().urgent);
        store.session_update("thread", serde_json::json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hello"}})).unwrap();
        events.try_recv().unwrap();
        let streamed = events.try_recv().unwrap();
        assert!(!streamed.urgent);
        let snapshot = store.snapshot("thread").unwrap();
        assert_eq!(streamed.session.unwrap().revision, snapshot.revision);
        assert_eq!(snapshot.entries[0].content["text"], "hello");
        store
            .update("thread", |thread| {
                thread.permissions.push(crate::facade::Permission {
                    id: "confirm".into(),
                    kind: "permission".into(),
                    request: serde_json::json!({}),
                });
            })
            .unwrap();
        events.try_recv().unwrap();
        assert!(events.try_recv().unwrap().urgent);
        assert_eq!(invalidations.load(Ordering::SeqCst), 1);
        store
            .update("thread", |thread| thread.title = "Renamed".into())
            .unwrap();
        events.try_recv().unwrap();
        assert_eq!(
            events.try_recv().unwrap().session.unwrap().info.title,
            "Renamed"
        );
        store
            .update("thread", |thread| {
                thread.status = "idle".into();
                thread.permissions.clear();
            })
            .unwrap();
        events.try_recv().unwrap();
        assert!(events.try_recv().unwrap().urgent);
        assert_eq!(invalidations.load(Ordering::SeqCst), 2);
        store.remove("thread").unwrap();
        assert_eq!(invalidations.load(Ordering::SeqCst), 3);
        assert!(events.try_recv().unwrap().deleted);
        assert!(events.try_recv().unwrap().deleted);
        assert!(matches!(
            events.try_recv().unwrap().resource,
            SyncResource::Sessions { .. }
        ));
        assert!(store.snapshot("thread").is_err());
        assert!(events.try_recv().is_err());
    }
}
