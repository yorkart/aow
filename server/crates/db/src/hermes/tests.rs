use super::*;

fn create_database(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(include_str!("sql/tests/create_legacy_database.sql"))?;
    Ok(connection)
}

#[test]
fn external_databases_remain_read_only_and_missing_paths_are_not_created() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing/state.db");
    assert!(HermesDb::open(&missing).is_err());
    assert!(!missing.parent().unwrap().exists());
    let path = directory.path().join("state.db");
    drop(create_database(&path)?);
    let original = std::fs::read(&path)?;
    let mut db = HermesDb::open(&path)?;
    assert_eq!(
        db.query_sessions(None, Some("/repo"), Some("/repo"))?.len(),
        1
    );
    assert_eq!(db.read()?.recent_messages("root", 200)?.len(), 2);
    let error = db
        .connection
        .execute(include_str!("sql/tests/delete_session.sql"), ["root"])
        .unwrap_err();
    assert!(
        matches!(error, rusqlite::Error::SqliteFailure(error, _) if error.code == rusqlite::ErrorCode::ReadOnly)
    );
    drop(db);
    assert_eq!(std::fs::read(&path)?, original);
    Ok(())
}

#[test]
fn read_transactions_keep_watermarks_and_history_consistent_across_wal_commits() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.db");
    let writer = create_database(&path)?;
    let mut db = HermesDb::open(&path)?;
    let reader = db.read()?;
    assert_eq!(reader.watermark()?, 2);
    writer.execute_batch(include_str!("sql/tests/append_messages.sql"))?;
    assert_eq!(reader.watermark()?, 2);
    assert!(!reader.session_exists("other")?);
    assert_eq!(reader.recent_messages("root", 200)?.len(), 2);
    assert!(reader.tail_messages("root", 2, 512)?.is_empty());
    assert_eq!(
        reader.last_message_timestamp("root")?,
        Some(1700000001.123456)
    );
    drop(reader);

    let reader = db.read()?;
    assert_eq!(reader.watermark()?, 5);
    assert!(reader.session_exists("other")?);
    assert_eq!(
        reader.last_message_timestamp("root")?,
        Some(1700000003.123456)
    );
    let recent = reader.recent_messages("root", 0)?;
    assert_eq!(recent.iter().map(|row| row.id).collect::<Vec<_>>(), [3, 4]);
    assert_eq!(reader.recent_messages("root", 1)?.len(), 4);
    let tail = reader.tail_messages("root", 2, 1)?;
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].id, 3);
    assert_eq!(reader.tail_messages("root", 3, 512)?.len(), 1);
    assert!(reader.tail_messages("root", 4, 512)?.is_empty());
    Ok(())
}

#[test]
fn legacy_columns_and_native_schema_changes_preserve_archive_and_compaction_flags() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.db");
    let writer = create_database(&path)?;
    let mut db = HermesDb::open(&path)?;
    let rows = db.query_sessions(None, Some("/alias"), Some("/repo"))?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title.as_deref(), Some("Native title"));
    assert_eq!(
        rows[0].first_user_message.as_deref(),
        Some("First question")
    );
    assert!(db.live_session_exists("root", "/repo")?);
    assert!(!db.live_session_exists("root", "/elsewhere")?);
    assert!(!db.live_session_exists("missing", "/repo")?);
    let reader = db.read()?;
    assert!(reader.compacted_messages("root")?.is_none());
    assert_eq!(
        reader.recent_messages("root", 200)?[0].timestamp,
        1700000000.123456
    );
    drop(reader);

    writer.execute_batch(include_str!("sql/tests/upgrade_schema.sql"))?;
    assert!(db.query_sessions(None, None, None)?.is_empty());
    assert_eq!(db.query_sessions(Some("root"), None, None)?.len(), 1);
    let reader = db.read()?;
    let rows = reader.compacted_messages("root")?.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].message.id, 1);
    assert!(!rows[0].active);
    assert!(rows[0].compacted);
    assert_eq!(rows[1].message.id, 2);
    assert!(!rows[1].active);
    assert!(!rows[1].compacted);
    assert!(reader.recent_messages("root", 200)?.is_empty());
    assert!(reader.compacted_messages("missing")?.is_none());
    Ok(())
}

#[test]
fn row_decode_failures_are_not_treated_as_empty_history() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.db");
    let writer = create_database(&path)?;
    writer.execute(include_str!("sql/tests/corrupt_timestamp.sql"), [])?;
    let mut db = HermesDb::open(&path)?;
    assert!(db.query_sessions(None, None, None).is_err());
    let reader = db.read()?;
    assert!(reader.recent_messages("root", 200).is_err());
    assert!(reader.tail_messages("root", 0, 512).is_err());
    assert!(!reader.session_exists("missing")?);
    assert!(reader.recent_messages("missing", 200)?.is_empty());
    Ok(())
}
