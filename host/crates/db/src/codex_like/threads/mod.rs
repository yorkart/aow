mod model;
pub use model::ThreadRecord;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row};

pub(super) fn has_name(connection: &Connection) -> bool {
    connection
        .query_row(include_str!("sql/has_name.sql"), [], |row| row.get(0))
        .unwrap_or(false)
}

// Recent Codex/Trae schemas separate the visible name from the first-message title.
fn display_title_column(connection: &Connection) -> &'static str {
    if has_name(connection) {
        include_str!("sql/display_title.sql")
    } else {
        "title"
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<ThreadRecord> {
    Ok(ThreadRecord {
        id: row.get(0)?,
        rollout_path: row.get(1)?,
        cwd: row.get(2)?,
        title: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub(super) fn list(connection: &Connection, cwd: &str) -> Result<Vec<ThreadRecord>> {
    let sql =
        include_str!("sql/list_sessions.sql").replace("{title}", display_title_column(connection));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([cwd], read)?;
    Ok(rows.filter_map(|row| row.ok()).collect())
}

pub(super) fn find<'a>(
    connection: &Connection,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<ThreadRecord>> {
    let sql =
        include_str!("sql/find_session.sql").replace("{title}", display_title_column(connection));
    let mut statement = connection.prepare(&sql)?;
    let mut rows = Vec::new();
    for id in ids {
        if let Some(row) = statement.query_row([id], read).optional()? {
            rows.push(row);
        }
    }
    Ok(rows)
}
