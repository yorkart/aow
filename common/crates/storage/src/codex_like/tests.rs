use super::*;

fn create_database(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(include_str!("sql/tests/create_database.sql"))
        .unwrap();
    connection
}

#[test]
fn external_databases_are_read_only_and_missing_paths_are_not_created() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing/state_5.sqlite");
    assert!(CodexLikeDb::open(&missing).is_err());
    assert!(!missing.parent().unwrap().exists());

    let path = directory.path().join("state_5.sqlite");
    let writer = create_database(&path);
    drop(writer);
    let original = std::fs::read(&path)?;
    let db = CodexLikeDb::open(&path)?;
    assert_eq!(db.list_threads("/repo")?.len(), 2);
    let error = db
        .connection
        .execute(include_str!("sql/tests/delete_thread.sql"), ["first"])
        .unwrap_err();
    assert!(
        matches!(error, rusqlite::Error::SqliteFailure(error, _) if error.code == rusqlite::ErrorCode::ReadOnly)
    );
    drop(db);
    assert_eq!(
        std::fs::read(&path)?,
        original,
        "opening and querying must not initialize or migrate an external database"
    );
    Ok(())
}

#[test]
fn native_names_follow_schema_changes_and_fall_back_for_null_or_empty_names() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state_5.sqlite");
    let writer = create_database(&path);
    let db = CodexLikeDb::open(&path)?;
    assert!(!db.has_thread_names());
    assert_eq!(db.find_threads(["first"])?[0].title, "First prompt");
    writer.execute_batch(include_str!("sql/tests/add_name.sql"))?;
    assert!(db.has_thread_names());
    let rows = db.find_threads(["second", "missing", "first"])?;
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert_eq!(rows[0].title, "Second prompt");
    assert_eq!(rows[1].title, "Native first");
    writer.execute(include_str!("sql/tests/rename_thread.sql"), ["", "first"])?;
    assert_eq!(db.find_threads(["first"])?[0].title, "First prompt");
    let rows = db.list_threads("/repo")?;
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert_eq!(rows[1].created_at, 1_725_100_000);
    assert_eq!(rows[1].updated_at, 1_725_100_123_000);
    Ok(())
}

#[test]
fn malformed_rows_are_skipped_in_lists_but_fail_explicit_lookups() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state_5.sqlite");
    let writer = create_database(&path);
    writer.execute(include_str!("sql/tests/corrupt_timestamp.sql"), ["first"])?;
    let db = CodexLikeDb::open(&path)?;
    let rows = db.list_threads("/repo")?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "second");
    assert!(db.find_threads(["first"]).is_err());
    assert!(db.find_threads(["second", "first"]).is_err());
    assert!(db.find_threads(["missing"])?.is_empty());
    assert_eq!(db.find_threads(["missing", "second"])?[0].id, "second");
    Ok(())
}
