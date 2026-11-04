//! AoW-owned ACP metadata and snapshot storage, scoped to one database.
mod acp_thread_snapshots;
mod migrations;
mod sidebar_threads;
#[cfg(test)]
mod tests;

pub use acp_thread_snapshots::SnapshotRecord;
pub use sidebar_threads::ThreadRecord;

use anyhow::Result;
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

/// Clones share one connection; opening again creates a separate connection.
#[derive(Clone)]
pub struct AcpDb {
    connection: Arc<Mutex<Connection>>,
}

/// The caller captures and encodes live state before entering the database transaction.
pub enum ThreadWrite<'a> {
    Upsert {
        metadata: &'a ThreadRecord,
        snapshot: &'a [u8],
    },
    Delete(&'a str),
}

impl AcpDb {
    /// Open and migrate an existing database, or create a private database.
    /// `None` creates an isolated in-memory database.
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let mut connection = crate::sqlite::open(path)?;
        migrations::apply(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// List metadata without reading or decoding snapshot bodies.
    pub fn list_threads(&self) -> Result<Vec<ThreadRecord>> {
        sidebar_threads::list(&self.connection.lock().unwrap())
    }

    pub fn load_snapshot(&self, id: &str) -> Result<Option<SnapshotRecord>> {
        acp_thread_snapshots::load(&self.connection.lock().unwrap(), id)
    }

    /// Commit the entire batch atomically, including both tables for each thread.
    /// Snapshot bytes use the caller's versioned JSON encoding.
    pub fn write(&self, operations: &[ThreadWrite<'_>]) -> Result<()> {
        if operations.is_empty() {
            return Ok(());
        }
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        for operation in operations {
            match operation {
                ThreadWrite::Upsert { metadata, snapshot } => {
                    sidebar_threads::save(&transaction, metadata)?;
                    acp_thread_snapshots::save(&transaction, &metadata.thread_id, snapshot)?;
                }
                ThreadWrite::Delete(id) => {
                    acp_thread_snapshots::delete(&transaction, id)?;
                    sidebar_threads::delete(&transaction, id)?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
