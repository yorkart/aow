use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// An opaque cached body; ACP owns its encoding version and session validation.
#[derive(Debug, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub data_type: String,
    pub data: Vec<u8>,
}

pub(super) fn load(connection: &Connection, id: &str) -> Result<Option<SnapshotRecord>> {
    Ok(connection
        .query_row(include_str!("sql/load_snapshot.sql"), [id], |row| {
            Ok(SnapshotRecord {
                data_type: row.get(0)?,
                data: row.get(1)?,
            })
        })
        .optional()?)
}

pub(super) fn save(connection: &Connection, id: &str, data: &[u8]) -> Result<()> {
    connection.execute(include_str!("sql/save_snapshot.sql"), params![id, data])?;
    Ok(())
}

pub(super) fn delete(connection: &Connection, id: &str) -> Result<()> {
    connection.execute(include_str!("sql/delete_snapshot.sql"), [id])?;
    Ok(())
}
