use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{os::unix::fs::OpenOptionsExt, path::Path, time::Duration};

/// Open an AoW-owned database, preserving private permissions on SQLite sidecars.
pub(crate) fn open(path: Option<&Path>) -> Result<Connection> {
    let connection = if let Some(path) = path {
        std::fs::create_dir_all(path.parent().context("Missing database directory")?)?;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        Connection::open(path)
            .with_context(|| format!("Cannot open database {}", path.display()))?
    } else {
        Connection::open_in_memory()?
    };
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(include_str!("sql/initialize.sql"))?;
    Ok(connection)
}
