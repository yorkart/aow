//! The upstream sidebar_threads schema and SQL operation boundaries.
mod model;
pub use model::ThreadRecord;

use anyhow::Result;
use rusqlite::{Connection, params};

pub(super) fn list(connection: &Connection) -> Result<Vec<ThreadRecord>> {
    let mut statement = connection.prepare(include_str!("sql/list.sql"))?;
    let rows = statement.query_map([], |row| {
        Ok(ThreadRecord {
            thread_id: row.get(0)?,
            session_id: row.get(1)?,
            agent_id: row.get(2)?,
            title: row.get(3)?,
            updated_at: row.get(4)?,
            created_at: row.get(5)?,
            interacted_at: row.get(6)?,
            folder_paths: row.get(7)?,
            archived: row.get(8)?,
            title_override: row.get(9)?,
            parent: row.get(10)?,
            source: row.get(11)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn save(connection: &Connection, row: &ThreadRecord) -> Result<()> {
    connection.execute(
        include_str!("sql/save.sql"),
        params![
            row.thread_id,
            row.session_id,
            row.agent_id,
            row.title,
            row.updated_at,
            row.created_at,
            row.interacted_at,
            row.folder_paths,
            row.archived,
            row.title_override,
            row.parent,
            row.source,
        ],
    )?;
    Ok(())
}

pub(super) fn delete(connection: &Connection, id: &str) -> Result<()> {
    connection.execute(include_str!("sql/delete.sql"), [id])?;
    Ok(())
}
