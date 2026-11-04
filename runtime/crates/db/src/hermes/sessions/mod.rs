mod model;
pub use model::SessionRecord;

use rusqlite::{Connection, Result, params};

use super::{history::compression_edges, messages, schema::has_column};

pub(super) fn query(
    connection: &Connection,
    id: Option<&str>,
    cwd: Option<&str>,
    canonical_cwd: Option<&str>,
) -> Result<Vec<SessionRecord>> {
    let archived = if has_column(connection, "sessions", "archived") {
        include_str!("sql/unarchived_sessions.sql")
    } else {
        ""
    };
    let sql = format!(
        include_str!("sql/sessions.sql"),
        edges = compression_edges(connection),
        visible = user_session_filter(connection, id.is_some()),
        active = messages::visible_lineage_filter(connection),
        archived = archived,
    );
    connection
        .prepare(&sql)?
        .query_map(params![id, cwd, canonical_cwd], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                cwd: row.get(1)?,
                title: row.get(2)?,
                started_at: row.get(3)?,
                updated_at: row.get(4)?,
                first_user_message: row.get(5)?,
            })
        })?
        .collect()
}

pub(super) fn exists(connection: &Connection, id: &str) -> Result<bool> {
    connection.query_row(include_str!("sql/session_exists.sql"), [id], |row| {
        row.get(0)
    })
}

pub(super) fn live_exists(connection: &Connection, id: &str, cwd: &str) -> Result<bool> {
    connection.query_row(
        include_str!("sql/live_session_exists.sql"),
        params![id, cwd],
        |row| row.get(0),
    )
}

fn user_session_filter(connection: &Connection, include_continuations: bool) -> String {
    let (branch, delegate) = if has_column(connection, "sessions", "model_config") {
        let config = include_str!("sql/session_model_config.sql");
        (
            format!(include_str!("sql/branch_filter.sql"), config = config),
            format!(include_str!("sql/delegate_filter.sql"), config = config),
        )
    } else {
        (String::new(), String::new())
    };
    let compression = if include_continuations {
        include_str!("sql/compression_filter.sql")
    } else {
        ""
    };
    format!(
        include_str!("sql/user_session_filter.sql"),
        delegate = delegate,
        branch = branch,
        compression = compression,
    )
}
