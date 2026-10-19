use rusqlite::{Connection, OpenFlags, Result};
use std::{path::Path, time::Duration};

/// Open an external database without creating files, configuring its journal or migrating it.
pub(crate) fn open(path: &Path, busy_timeout: Duration) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(busy_timeout)?;
    Ok(connection)
}
