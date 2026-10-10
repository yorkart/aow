//! Small AoW host adapter for Zed's ThreadSafeConnection/Domain migration boundary.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

pub(crate) type ThreadSafeConnection = Arc<Mutex<Connection>>;

pub(crate) fn open(path: Option<&Path>) -> Result<ThreadSafeConnection> {
    let connection = if let Some(path) = path {
        std::fs::create_dir_all(path.parent().context("Missing ACP database directory")?)?;
        // Match the private permissions of the former JSON snapshots, including SQLite sidecars.
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        Connection::open(path)
            .with_context(|| format!("Cannot open ACP database {}", path.display()))?
    } else {
        Connection::open_in_memory()?
    };
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
        CREATE TABLE IF NOT EXISTS schema_migrations(domain TEXT PRIMARY KEY NOT NULL, version INTEGER NOT NULL) STRICT;")?;
    Ok(Arc::new(Mutex::new(connection)))
}

pub(crate) fn migration_version(connection: &Connection, domain: &str) -> Result<usize> {
    Ok(connection
        .query_row(
            "SELECT version FROM schema_migrations WHERE domain = ?",
            [domain],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub(crate) fn set_migration_version(
    connection: &Connection,
    domain: &str,
    version: usize,
) -> Result<()> {
    connection.execute(
        "INSERT INTO schema_migrations(domain, version) VALUES (?1, ?2)
        ON CONFLICT(domain) DO UPDATE SET version = excluded.version",
        params![domain, version],
    )?;
    Ok(())
}

/// Equivalent of sqlez::Connection::migrate(domain, migrations), without GPUI globals.
pub(crate) fn migrate(
    connection: &mut Connection,
    domain: &str,
    migrations: &[&str],
) -> Result<()> {
    let transaction = connection.transaction()?;
    let version = migration_version(&transaction, domain)?;
    ensure!(
        version <= migrations.len(),
        "ACP database domain {domain} is newer than this application"
    );
    for migration in &migrations[version..] {
        transaction.execute_batch(migration)?;
    }
    set_migration_version(&transaction, domain, migrations.len())?;
    transaction.commit()?;
    Ok(())
}
