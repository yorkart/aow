//! Host port of agent_ui::thread_metadata_store: cached metadata, queued operations and SQLite.
mod db;
mod migration;
mod operations;
#[cfg(test)]
mod tests;

use self::{
    db::ThreadMetadataDb,
    operations::{DbOperation, DbOperations},
};
use crate::{acp_thread::snapshot_store::SnapshotDb, facade::SessionSnapshot};
use anyhow::Result;
use std::{collections::HashMap, path::Path};

// GPUI IDs, SharedString, DateTime and PathList are facade strings/paths in this host.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ThreadMetadata {
    pub thread_id: String,
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
            thread_id: snapshot.id.clone(),
            session_id: Some(snapshot.remote_id.clone()),
            agent_id: snapshot.agent_id.clone(),
            title: snapshot.title.clone(),
            updated_at: snapshot.updated_at.clone(),
            created_at: previous
                .and_then(|row| row.created_at.clone())
                .or_else(|| Some(snapshot.updated_at.clone())),
            interacted_at: previous.and_then(|row| row.interacted_at.clone()),
            folder_paths: vec![snapshot.cwd.clone()],
            archived: previous.is_some_and(|row| row.archived),
            title_override: previous.and_then(|row| row.title_override.clone()),
        }
    }
}

pub(crate) struct ThreadMetadataStore {
    db: ThreadMetadataDb,
    threads: HashMap<String, ThreadMetadata>,
    pending_thread_ops: DbOperations,
}
impl ThreadMetadataStore {
    pub(crate) fn new(state_directory: Option<&Path>) -> Result<Self> {
        let path = state_directory.map(|directory| directory.join("threads.sqlite"));
        let db = ThreadMetadataDb(crate::db::open(path.as_deref())?);
        {
            let mut connection = db.0.lock().unwrap();
            crate::db::migrate(
                &mut connection,
                ThreadMetadataDb::NAME,
                ThreadMetadataDb::MIGRATIONS,
            )?;
            crate::db::migrate(&mut connection, SnapshotDb::NAME, SnapshotDb::MIGRATIONS)?;
            migration::migrate_json_history(&mut connection, state_directory)?;
        }
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
            .list()?
            .into_iter()
            .map(|row| (row.thread_id.clone(), row))
            .collect();
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
        // Unlike Zed's global session-ID index, adapters may reuse the same ID in AoW.
        self.entries().find(|row| {
            row.agent_id == agent_id
                && row.cwd() == cwd
                && row.session_id.as_deref() == Some(session_id)
        })
    }
    pub(crate) fn save_all(&mut self, snapshots: &[SessionSnapshot]) {
        for snapshot in snapshots {
            self.save(snapshot);
        }
    }
    pub(crate) fn save(&mut self, snapshot: &SessionSnapshot) {
        let metadata = ThreadMetadata::from_snapshot(snapshot, self.entry(&snapshot.id));
        self.save_internal(metadata, snapshot.clone());
    }
    fn save_internal(&mut self, metadata: ThreadMetadata, snapshot: SessionSnapshot) {
        self.threads
            .insert(metadata.thread_id.clone(), metadata.clone());
        self.pending_thread_ops
            .enqueue(DbOperation::Upsert(Box::new(metadata), Box::new(snapshot)));
    }
    pub(crate) fn delete(&mut self, thread_id: &str) {
        self.threads.remove(thread_id);
        self.pending_thread_ops
            .enqueue(DbOperation::Delete(thread_id.into()));
    }
    pub(crate) fn load_thread(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        SnapshotDb::load_thread(&self.db.0.lock().unwrap(), id)
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
