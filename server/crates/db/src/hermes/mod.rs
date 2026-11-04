//! Read-only access to Hermes' native SQLite session and message history.
mod history;
mod messages;
mod reader;
mod schema;
mod sessions;
#[cfg(test)]
mod tests;

pub use messages::{CompactedMessageRecord, MessageRecord};
pub use reader::HermesRead;
pub use sessions::SessionRecord;

use anyhow::Result;
use rusqlite::Connection;
use std::{path::Path, time::Duration};

/// An external Hermes database. Opening it never initializes or migrates the schema.
pub struct HermesDb {
    connection: Connection,
}

impl HermesDb {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            connection: crate::sqlite::open_read_only(path, Duration::from_millis(250))?,
        })
    }

    /// Read user conversations, optionally restricted to an ID or workspace.
    /// The caller supplies both the original and canonical workspace paths.
    pub fn query_sessions(
        &self,
        id: Option<&str>,
        cwd: Option<&str>,
        canonical_cwd: Option<&str>,
    ) -> Result<Vec<SessionRecord>> {
        Ok(sessions::query(&self.connection, id, cwd, canonical_cwd)?)
    }

    pub fn live_session_exists(&self, id: &str, cwd: &str) -> Result<bool> {
        Ok(sessions::live_exists(&self.connection, id, cwd)?)
    }

    /// Keep related queries in one SQLite read transaction, including WAL reads.
    pub fn read(&mut self) -> Result<HermesRead<'_>> {
        Ok(HermesRead::new(&mut self.connection)?)
    }
}
