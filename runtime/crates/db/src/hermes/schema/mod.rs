use rusqlite::{Connection, params};

pub(super) fn has_column(connection: &Connection, table: &str, column: &str) -> bool {
    connection
        .query_row(
            include_str!("sql/has_column.sql"),
            params![table, column],
            |row| row.get(0),
        )
        .unwrap_or(false)
}
