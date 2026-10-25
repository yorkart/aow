//! Host port of agent_ui::thread_metadata_store: cached metadata, queued operations and SQLite.
mod operations;
mod record;
#[cfg(test)]
mod tests;

use self::operations::{DbOperation, DbOperations};
use crate::{acp_thread::snapshot_store, facade::SessionSnapshot};
use anyhow::Result;
use aow_db::acp::{AcpDb, ThreadWrite};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

// GPUI IDs, SharedString, DateTime and PathList are facade strings/paths in this host.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ThreadMetadata {
    pub thread_id: String,
    pub source: crate::facade::SessionSource,
    pub parent: Option<aow_protocol::InstanceRef>,
    pub session_id: Option<String>,
    pub agent_id: String,
    pub title: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub interacted_at: Option<String>,
    pub folder_paths: Vec<String>,
    pub archived: bool,
    pub title_override: Option<String>,
}
impl ThreadMetadata {
    pub(crate) fn display_title(&self) -> &str {
        self.title_override.as_deref().unwrap_or(&self.title)
    }
    pub(crate) fn cwd(&self) -> &str {
        self.folder_paths
            .first()
            .map(String::as_str)
            .unwrap_or_default()
    }
    fn from_snapshot(snapshot: &SessionSnapshot, previous: Option<&Self>) -> Self {
        Self {
            thread_id: snapshot.thread_id.clone(),
            source: snapshot.source,
            parent: snapshot.parent.clone(),
            session_id: snapshot.session_id.clone(),
            agent_id: snapshot.agent_id.clone(),
            title: snapshot.title.clone(),
            updated_at: snapshot.updated_at.clone(),
            created_at: Some(snapshot.created_at.clone()),
            interacted_at: snapshot.interacted_at.clone(),
            folder_paths: vec![snapshot.cwd.clone()],
            archived: previous.is_some_and(|row| row.archived),
            title_override: previous.and_then(|row| row.title_override.clone()),
        }
    }
}

pub(crate) struct ThreadMetadataStore {
    db: AcpDb,
    threads: HashMap<String, ThreadMetadata>,
    pending_thread_ops: DbOperations,
}
impl ThreadMetadataStore {
    pub(crate) fn new(state_directory: Option<&Path>) -> Result<Self> {
        let path = state_directory.map(|directory| directory.join("threads.sqlite"));
        let db = AcpDb::open(path.as_deref())?;
        let mut this = Self {
            pending_thread_ops: DbOperations::new(db.clone())?,
            db,
            threads: HashMap::new(),
        };
        this.reload()?;
        Ok(this)
    }
    fn reload(&mut self) -> Result<()> {
        self.threads = self
            .db
            .list_threads()?
            .into_iter()
            .map(|row| {
                let metadata = ThreadMetadata::try_from(row)?;
                Ok((metadata.thread_id.clone(), metadata))
            })
            .collect::<Result<_>>()?;
        Ok(())
    }
    pub(crate) fn entries(&self) -> impl Iterator<Item = &ThreadMetadata> {
        self.threads.values()
    }
    pub(crate) fn entry(&self, id: &str) -> Option<&ThreadMetadata> {
        self.threads.get(id)
    }
    pub(crate) fn entry_by_session(
        &self,
        agent_id: &str,
        cwd: &str,
        session_id: &str,
    ) -> Option<&ThreadMetadata> {
        // Unlike the upstream implementation's global session-ID index, adapters may reuse the same ID in AoW.
        self.entries().find(|row| {
            row.agent_id == agent_id
                && row.cwd() == cwd
                && row.session_id.as_deref() == Some(session_id)
        })
    }
    pub(crate) fn save(&mut self, source: &Arc<Mutex<SessionSnapshot>>) {
        let snapshot = source.lock().unwrap();
        let metadata = ThreadMetadata::from_snapshot(&snapshot, self.entry(&snapshot.thread_id));
        drop(snapshot);
        self.threads
            .insert(metadata.thread_id.clone(), metadata.clone());
        // Queue a handle, not a deep copy. The writer captures the latest state
        // once per checkpoint, after notifications have coalesced.
        self.pending_thread_ops.enqueue(DbOperation {
            metadata: Box::new(metadata),
            snapshot: source.clone(),
        });
    }
    /// The caller holds ThreadStore's lock, excluding concurrent saves for this ID.
    /// Drain earlier writes, then delete synchronously: a failed deletion must not
    /// enter the background retry queue or run after the caller reports failure.
    pub(crate) fn delete_durable(&mut self, id: &str) -> Result<()> {
        self.pending_thread_ops.flush()?;
        self.db.write(&[ThreadWrite::Delete(id)])?;
        self.threads.remove(id);
        Ok(())
    }
    pub(crate) fn load_thread(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        self.db
            .load_snapshot(id)?
            .map(|row| snapshot_store::decode(id, row))
            .transpose()
    }
    pub(crate) fn flush(&self) -> Result<()> {
        self.pending_thread_ops.flush()
    }
    fn dedup_db_operations(operations: Vec<DbOperation>) -> Vec<DbOperation> {
        let mut ops = HashMap::new();
        for operation in operations.into_iter().rev() {
            if ops.contains_key(operation.id()) {
                continue;
            }
            ops.insert(operation.id().to_owned(), operation);
        }
        ops.into_values().collect()
    }
}
