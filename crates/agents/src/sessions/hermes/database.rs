use super::*;

pub(in crate::sessions) fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
    Ok(connection)
}

pub(in crate::sessions) fn has_column(connection: &Connection, table: &str, column: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            params![table, column],
            |row| row.get(0),
        )
        .unwrap_or(false)
}

pub(in crate::sessions) fn timestamp(epoch: f64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis((epoch * 1000.0) as i64).unwrap_or(DateTime::UNIX_EPOCH)
}
