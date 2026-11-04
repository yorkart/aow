mod model;
pub use model::{CompactedMessageRecord, MessageRecord};

use rusqlite::{Connection, Result, Row};

use super::schema::has_column;

pub(super) fn read(row: &Row<'_>) -> Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        tool_call_id: row.get(4)?,
        tool_calls: row.get(5)?,
        timestamp: row.get(6)?,
        finish_reason: row.get(7)?,
    })
}

pub(super) fn read_compacted(row: &Row<'_>) -> Result<CompactedMessageRecord> {
    Ok(CompactedMessageRecord {
        message: read(row)?,
        active: row.get(8)?,
        compacted: row.get(9)?,
    })
}

pub(super) fn watermark(connection: &Connection) -> Result<i64> {
    connection.query_row(include_str!("sql/watermark.sql"), [], |row| row.get(0))
}

pub(super) fn active_filter(connection: &Connection) -> &'static str {
    if has_column(connection, "messages", "active") {
        include_str!("sql/active_messages.sql")
    } else {
        ""
    }
}

pub(super) fn visible_lineage_filter(connection: &Connection) -> &'static str {
    if has_column(connection, "messages", "compacted") {
        include_str!("sql/visible_messages.sql")
    } else if has_column(connection, "messages", "active") {
        include_str!("sql/active_lineage_messages.sql")
    } else {
        ""
    }
}
