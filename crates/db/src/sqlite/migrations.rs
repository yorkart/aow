//! Domain migrations adapted from the upstream sqlez connection boundary.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn migrate(
    connection: &mut Connection,
    domain: &str,
    migrations: &[&str],
) -> Result<()> {
    let transaction = connection.transaction()?;
    let version: usize = transaction
        .query_row(include_str!("sql/migration_version.sql"), [domain], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or(0);
    ensure!(
        version <= migrations.len(),
        "ACP database domain {domain} is newer than this application"
    );
    for migration in &migrations[version..] {
        transaction.execute_batch(migration)?;
    }
    transaction.execute(
        include_str!("sql/set_migration_version.sql"),
        params![domain, migrations.len()],
    )?;
    transaction.commit()?;
    Ok(())
}
