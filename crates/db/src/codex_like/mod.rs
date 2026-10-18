//! Read-only access to the compatible Codex and Trae SQLite session indexes.
#[cfg(test)]
mod tests;
mod threads;

pub use threads::ThreadRecord;

use anyhow::Result;
use rusqlite::Connection;
use std::{path::Path, time::Duration};

/// An external database at the caller-selected path. No schema initialization or migrations run.
pub struct CodexLikeDb {
    connection: Connection,
}

impl CodexLikeDb {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            connection: crate::sqlite::open_read_only(path, Duration::from_millis(250))?,
        })
    }

    /// Older schemas keep user-visible names outside SQLite; callers supply that fallback.
    pub fn has_thread_names(&self) -> bool {
        threads::has_name(&self.connection)
    }

    /// List top-level, non-archived conversations for one workspace, skipping malformed rows.
    pub fn list_threads(&self, cwd: &str) -> Result<Vec<ThreadRecord>> {
        threads::list(&self.connection, cwd)
    }

    /// Find IDs in input order, without workspace or sidebar filters.
    /// Missing IDs are omitted; database and row decoding failures are returned to the caller.
    pub fn find_threads<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<ThreadRecord>> {
        threads::find(&self.connection, ids)
    }
}
