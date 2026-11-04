use anyhow::Result;
use aow_sqlite::{migrate, open, open_read_only};
use rusqlite::{Connection, Error, ErrorCode};
use std::time::Duration;

fn version(connection: &Connection, domain: &str) -> Result<i64> {
    Ok(connection.query_row(
        "SELECT version FROM schema_migrations WHERE domain = ?1",
        [domain],
        |row| row.get(0),
    )?)
}

#[test]
fn owned_connections_preserve_data_and_private_file_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/owned.sqlite");
    let mut connection = open(Some(&path))?;
    migrate(
        &mut connection,
        "Example",
        &["CREATE TABLE records(value TEXT); INSERT INTO records VALUES ('saved');"],
    )?;
    for file in [
        &path,
        &path.with_extension("sqlite-wal"),
        &path.with_extension("sqlite-shm"),
    ] {
        assert_eq!(std::fs::metadata(file)?.permissions().mode() & 0o777, 0o600);
    }
    drop(connection);
    let connection = open(Some(&path))?;
    assert_eq!(
        connection.query_row("SELECT value FROM records", [], |row| row
            .get::<_, String>(0))?,
        "saved"
    );
    assert_eq!(version(&connection, "Example")?, 1);
    assert_eq!(
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
        "wal"
    );
    for (pragma, expected) in [
        ("PRAGMA foreign_keys", 1),
        ("PRAGMA synchronous", 2),
        ("PRAGMA busy_timeout", 5000),
    ] {
        assert_eq!(
            connection.query_row(pragma, [], |row| row.get::<_, i64>(0))?,
            expected
        );
    }
    Ok(())
}

#[test]
fn migration_domains_advance_independently_and_repeated_opens_do_not_reapply_sql() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("owned.sqlite");
    let mut connection = open(Some(&path))?;
    let first = "CREATE TABLE records(value INTEGER); INSERT INTO records VALUES (1);";
    let second = "INSERT INTO records VALUES (2);";
    migrate(&mut connection, "One", &[first])?;
    migrate(&mut connection, "Two", &["CREATE TABLE other(value TEXT);"])?;
    drop(connection);
    let mut connection = open(Some(&path))?;
    migrate(&mut connection, "One", &[first, second])?;
    migrate(&mut connection, "One", &[first, second])?;
    assert_eq!(version(&connection, "One")?, 2);
    assert_eq!(version(&connection, "Two")?, 1);
    assert_eq!(
        connection.query_row("SELECT SUM(value) FROM records", [], |row| row
            .get::<_, i64>(0))?,
        3
    );
    Ok(())
}

#[test]
fn failed_migrations_roll_back_schema_data_and_version_before_retry() -> Result<()> {
    let mut connection = open(None)?;
    let first = "CREATE TABLE records(value INTEGER); INSERT INTO records VALUES (1);";
    let next = "CREATE TABLE added(value TEXT); INSERT INTO records VALUES (2);";
    migrate(&mut connection, "Example", &[first])?;
    assert!(
        migrate(
            &mut connection,
            "Example",
            &[first, next, "INSERT INTO missing VALUES (3);"]
        )
        .is_err()
    );
    assert_eq!(version(&connection, "Example")?, 1);
    assert_eq!(
        connection.query_row("SELECT SUM(value) FROM records", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'added'",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    migrate(&mut connection, "Example", &[first, next])?;
    assert_eq!(version(&connection, "Example")?, 2);
    assert_eq!(
        connection.query_row("SELECT SUM(value) FROM records", [], |row| row
            .get::<_, i64>(0))?,
        3
    );
    Ok(())
}

#[test]
fn newer_domains_are_rejected_without_changing_the_database() -> Result<()> {
    let mut connection = open(None)?;
    let first = "CREATE TABLE records(value INTEGER);";
    migrate(
        &mut connection,
        "Example",
        &[first, "INSERT INTO records VALUES (7);"],
    )?;
    let error = migrate(&mut connection, "Example", &[first]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Database domain Example is newer than this application"
    );
    assert_eq!(version(&connection, "Example")?, 2);
    assert_eq!(
        connection.query_row("SELECT value FROM records", [], |row| row.get::<_, i64>(0))?,
        7
    );
    Ok(())
}

#[test]
fn in_memory_connections_are_isolated() -> Result<()> {
    let mut first = open(None)?;
    migrate(
        &mut first,
        "Example",
        &["CREATE TABLE records(value INTEGER);"],
    )?;
    let second = open(None)?;
    assert_eq!(
        second.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    assert!(second.prepare("SELECT * FROM records").is_err());
    Ok(())
}

#[test]
fn external_connections_do_not_create_initialize_migrate_or_write_files() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing/external.sqlite");
    let timeout = Duration::from_millis(250);
    assert!(open_read_only(&missing, timeout).is_err());
    assert!(!missing.parent().unwrap().exists());
    let path = directory.path().join("external.sqlite");
    let writer = Connection::open(&path)?;
    writer.execute_batch(
        "CREATE TABLE records(value TEXT); INSERT INTO records VALUES ('external');",
    )?;
    drop(writer);
    let original = std::fs::read(&path)?;
    let connection = open_read_only(&path, timeout)?;
    assert_eq!(
        connection.query_row("SELECT value FROM records", [], |row| row
            .get::<_, String>(0))?,
        "external"
    );
    assert_eq!(
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
        "delete"
    );
    assert_eq!(
        connection.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))?,
        250
    );
    assert!(
        connection
            .prepare("SELECT * FROM schema_migrations")
            .is_err()
    );
    let error = connection.execute("DELETE FROM records", []).unwrap_err();
    assert!(matches!(error, Error::SqliteFailure(error, _) if error.code == ErrorCode::ReadOnly));
    drop(connection);
    assert_eq!(std::fs::read(&path)?, original);
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}
