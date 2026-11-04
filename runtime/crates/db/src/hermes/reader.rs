use anyhow::Result;
use rusqlite::{Connection, Transaction};

use super::{CompactedMessageRecord, MessageRecord, history, messages, sessions};

/// Queries share the snapshot established by the first read until this value is dropped.
/// Connections, SQL and row decoding remain inside the database crate.
pub struct HermesRead<'connection> {
    transaction: Transaction<'connection>,
}

impl<'connection> HermesRead<'connection> {
    pub(super) fn new(connection: &'connection mut Connection) -> rusqlite::Result<Self> {
        Ok(Self {
            transaction: connection.transaction()?,
        })
    }

    pub fn session_exists(&self, id: &str) -> Result<bool> {
        Ok(sessions::exists(&self.transaction, id)?)
    }

    /// Highest message ID across the database, including other conversations.
    pub fn watermark(&self) -> Result<i64> {
        Ok(messages::watermark(&self.transaction)?)
    }

    pub fn last_message_timestamp(&self, id: &str) -> Result<Option<f64>> {
        Ok(history::last_message_timestamp(&self.transaction, id)?)
    }

    /// All rows needed to reconstruct a compacted conversation, including inactive copies.
    /// None means that the conversation has no native compaction history.
    pub fn compacted_messages(&self, id: &str) -> Result<Option<Vec<CompactedMessageRecord>>> {
        Ok(history::compacted_messages(&self.transaction, id)?)
    }

    /// Read from the Nth preceding user message, retaining one extra turn when requested
    /// by the caller. Compaction reconstruction and its limits belong to the caller.
    pub fn recent_messages(
        &self,
        id: &str,
        preceding_user_turns: usize,
    ) -> Result<Vec<MessageRecord>> {
        Ok(history::recent_messages(
            &self.transaction,
            id,
            preceding_user_turns,
        )?)
    }

    pub fn tail_messages(&self, id: &str, cursor: i64, limit: usize) -> Result<Vec<MessageRecord>> {
        Ok(history::tail_messages(
            &self.transaction,
            id,
            cursor,
            limit,
        )?)
    }
}
