use super::*;
use crate::{
    acp_thread::{AcpThread, history::ThreadStore, snapshot_store::SnapshotDb},
    facade::{Permission, ThreadEntry},
};
use rusqlite::Connection;
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

fn thread(id: &str) -> SessionSnapshot {
    let mut thread = AcpThread::new_snapshot(
        id.into(),
        format!("remote-{id}"),
        "test".into(),
        "/repo".into(),
    );
    thread.title = format!("Conversation {id}");
    thread.updated_at = "2020-01-01T00:00:00+00:00".into();
    thread.entries = vec![ThreadEntry {
        id: "reply".into(),
        kind: "assistant".into(),
        content: json!({"type":"text","text":"original"}),
    }];
    thread
}
fn upsert(snapshot: SessionSnapshot) -> DbOperation {
    DbOperation::Upsert(
        Box::new(ThreadMetadata::from_snapshot(&snapshot, None)),
        Box::new(snapshot),
    )
}
fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

// Corresponding upstream tests: test_dedup_db_operations_keeps_latest_operation_for_session,
// test_dedup_db_operations_keeps_latest_insert_for_same_session, and preserves_distinct_sessions.
#[test]
fn dedup_db_operations_keeps_latest_operation_for_session() {
    let operations = ThreadMetadataStore::dedup_db_operations(vec![
        upsert(thread("a")),
        DbOperation::Delete("a".into()),
    ]);
    assert!(matches!(&operations[..], [DbOperation::Delete(id)] if id == "a"));
}
#[test]
fn dedup_db_operations_keeps_latest_insert_and_distinct_sessions() {
    let mut latest = thread("a");
    latest.title = "Latest".into();
    let operations = ThreadMetadataStore::dedup_db_operations(vec![
        upsert(thread("a")),
        upsert(thread("b")),
        upsert(latest),
    ]);
    assert_eq!(operations.len(), 2);
    assert!(operations.iter().any(|op| matches!(op, DbOperation::Upsert(metadata, snapshot) if metadata.thread_id == "a" && metadata.title == "Latest" && snapshot.title == "Latest")));
    assert!(operations.iter().any(|op| op.id() == "b"));
}

#[test]
fn sqlite_round_trip_keeps_metadata_and_content_without_json_rewrites() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    let mut snapshot = thread("a");
    snapshot.plan = json!({"entries":[{"content":"task","status":"in_progress"}]});
    snapshot.notices.push(ThreadEntry {
        id: "notice".into(),
        kind: "notice".into(),
        content: json!({"title":"Notice"}),
    });
    snapshot
        .terminals
        .insert("terminal".into(), json!({"output":"ANSI \u{1b}[32mhello"}));
    snapshot.config_options = json!([{ "id":"model", "currentValue":"test" }]);
    snapshot.permissions.push(Permission {
        id: "pending".into(),
        kind: "permission".into(),
        request: json!({}),
    });
    store.insert_metadata(snapshot)?;
    let expected = store.snapshot("a")?;
    assert!(!directory.path().join("history").exists());
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    assert_eq!(count(&connection, "sidebar_threads"), 1);
    assert_eq!(count(&connection, "acp_thread_snapshots"), 1);
    let journal: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    assert_eq!(journal, "wal");
    for name in ["threads.sqlite", "threads.sqlite-wal", "threads.sqlite-shm"] {
        assert_eq!(
            std::fs::metadata(directory.path().join(name))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let mut restored = ThreadStore::new(Some(directory.path().into()))?;
    assert!(restored.threads.is_empty());
    let sessions = restored.sessions(Some("/repo"));
    assert_eq!(sessions.len(), 1);
    assert!(restored.sessions(Some("/elsewhere")).is_empty());
    assert!(
        restored.threads.is_empty(),
        "listing must not hydrate transcripts"
    );
    let actual = restored.snapshot("a")?;
    assert_eq!(actual.status, "disconnected");
    assert!(actual.permissions.is_empty());
    assert_eq!(actual.updated_at, expected.updated_at);
    assert_eq!(actual.revision, expected.revision);
    assert_eq!(
        serde_json::to_value(actual.entries)?,
        serde_json::to_value(expected.entries)?
    );
    assert_eq!(actual.plan, expected.plan);
    assert_eq!(actual.terminals, expected.terminals);
    assert_eq!(actual.config_options, expected.config_options);
    assert_eq!(
        serde_json::to_value(actual.notices)?,
        serde_json::to_value(expected.notices)?
    );
    Ok(())
}

#[test]
fn unread_or_corrupt_body_does_not_break_the_metadata_archive() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    store.insert(thread("good"))?;
    store.insert(thread("bad"))?;
    {
        let connection = store.metadata.db.0.lock().unwrap();
        connection.execute(
            "UPDATE acp_thread_snapshots SET data = x'00' WHERE thread_id = 'bad'",
            [],
        )?;
    }
    let mut restored = ThreadStore::new(Some(directory.path().into()))?;
    assert_eq!(restored.sessions(None).len(), 2);
    assert!(restored.threads.is_empty());
    assert!(restored.snapshot("bad").is_err());
    assert_eq!(
        restored.snapshot("good")?.entries[0].content["text"],
        "original"
    );
    assert_eq!(restored.sessions(None).len(), 2);
    Ok(())
}

#[test]
fn migrate_json_history_once_atomically_and_never_resurrect_deleted_sessions() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let legacy = directory.path().join("history");
    std::fs::create_dir(&legacy)?;
    let mut imported = thread("imported");
    imported.needs_load = true;
    imported.entries.clear();
    for snapshot in [thread("a"), imported] {
        std::fs::write(
            legacy.join(format!("{}.json", snapshot.id)),
            serde_json::to_vec(&snapshot)?,
        )?;
    }
    std::fs::write(legacy.join("corrupt.json"), "{truncated")?;
    assert!(ThreadStore::new(Some(directory.path().into())).is_err());
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    assert_eq!(count(&connection, "sidebar_threads"), 0);
    assert_eq!(count(&connection, "acp_thread_snapshots"), 0);
    assert_eq!(
        crate::db::migration_version(&connection, "AowJsonHistory")?,
        0
    );
    std::fs::remove_file(legacy.join("corrupt.json"))?;
    let original = std::fs::read(legacy.join("a.json"))?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    assert_eq!(store.sessions(None).len(), 2);
    assert_eq!(store.snapshot("a")?.updated_at, thread("a").updated_at);
    assert!(store.snapshot("imported")?.needs_load);
    assert_eq!(
        std::fs::read(legacy.join("a.json"))?,
        original,
        "retain the migration backup"
    );
    store.update("a", |thread| thread.title = "New title".into())?;
    store.remove("imported")?;
    // Even a now-corrupt backup is ignored after the successful migration marker.
    std::fs::write(legacy.join("a.json"), "bad backup")?;
    let mut restored = ThreadStore::new(Some(directory.path().into()))?;
    assert_eq!(restored.sessions(None).len(), 1);
    assert_eq!(restored.snapshot("a")?.title, "New title");
    assert!(restored.snapshot("imported").is_err());
    assert_eq!(
        crate::db::migration_version(&connection, "AowJsonHistory")?,
        1
    );
    Ok(())
}

#[test]
fn streaming_saves_coalesce_and_turn_end_flushes_latest_content() -> Result<()> {
    let mut store = ThreadStore::new(None)?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    let db = store.metadata.db.0.clone();
    let connection = db.lock().unwrap();
    connection.execute_batch("CREATE TABLE save_counts(id INTEGER);
        CREATE TRIGGER count_snapshots AFTER UPDATE ON acp_thread_snapshots BEGIN INSERT INTO save_counts VALUES (1); END;")?;
    // Hold SQLite while notifications arrive: the pending queue must retain only the latest state.
    for index in 0..200 {
        store.update("a", |thread| {
            thread.entries[0].content["text"] = format!("chunk-{index}").into()
        })?;
    }
    drop(connection);
    store.update("a", |thread| thread.status = "idle".into())?;
    let connection = db.lock().unwrap();
    let saved = SnapshotDb::load_thread(&connection, "a")?.unwrap();
    assert_eq!(saved.status, "idle");
    assert_eq!(saved.entries[0].content["text"], "chunk-199");
    assert!(
        count(&connection, "save_counts") <= 2,
        "streaming must not write once per chunk"
    );
    Ok(())
}

#[test]
fn background_checkpoint_and_drop_preserve_in_progress_content() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    store.update("a", |thread| {
        thread.entries[0].content["text"] = "checkpoint".into()
    })?;
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if SnapshotDb::load_thread(&connection, "a")?.unwrap().entries[0].content["text"]
            == "checkpoint"
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background checkpoint did not run"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store.update("a", |thread| {
        thread.entries[0].content["text"] = "at shutdown".into()
    })?;
    drop(store);
    assert_eq!(
        SnapshotDb::load_thread(&connection, "a")?.unwrap().entries[0].content["text"],
        "at shutdown"
    );
    Ok(())
}

#[test]
fn database_failure_rolls_back_metadata_and_body_and_retry_keeps_latest_state() -> Result<()> {
    let mut store = ThreadStore::new(None)?;
    store.insert(thread("a"))?;
    let db = store.metadata.db.0.clone();
    db.lock().unwrap().execute_batch("CREATE TRIGGER reject_snapshot BEFORE INSERT ON acp_thread_snapshots BEGIN SELECT RAISE(ABORT, 'test write failure'); END;")?;
    assert!(
        store
            .update("a", |thread| {
                thread.title = "failed".into();
                thread.entries[0].content["text"] = "failed".into();
            })
            .is_err()
    );
    {
        let connection = db.lock().unwrap();
        assert_eq!(
            connection.query_row(
                "SELECT title FROM sidebar_threads WHERE thread_id = 'a'",
                [],
                |row| row.get::<_, String>(0)
            )?,
            "Conversation a"
        );
        assert_eq!(
            SnapshotDb::load_thread(&connection, "a")?.unwrap().entries[0].content["text"],
            "original"
        );
        connection.execute_batch("DROP TRIGGER reject_snapshot")?;
    }
    store.update("a", |thread| {
        thread.title = "latest".into();
        thread.entries[0].content["text"] = "latest".into();
    })?;
    assert_eq!(
        SnapshotDb::load_thread(&db.lock().unwrap(), "a")?
            .unwrap()
            .title,
        "latest"
    );
    Ok(())
}

#[test]
fn queued_updates_cannot_resurrect_a_deleted_thread() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    let db = store.metadata.db.0.clone();
    let connection = db.lock().unwrap();
    for index in 0..50 {
        store.update("a", |thread| thread.title = format!("update {index}"))?;
    }
    drop(connection);
    store.remove("a")?;
    store.metadata.flush()?;
    assert!(store.sessions(None).is_empty());
    let connection = db.lock().unwrap();
    assert_eq!(count(&connection, "sidebar_threads"), 0);
    assert_eq!(count(&connection, "acp_thread_snapshots"), 0);
    drop(connection);
    assert!(
        ThreadStore::new(Some(directory.path().into()))?
            .sessions(None)
            .is_empty()
    );
    Ok(())
}

#[test]
fn future_schema_and_corrupt_database_fail_without_silent_reset() -> Result<()> {
    let directory = tempfile::tempdir()?;
    drop(ThreadStore::new(Some(directory.path().into()))?);
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    connection.execute(
        "UPDATE schema_migrations SET version = 999 WHERE domain = 'ThreadMetadataDb'",
        [],
    )?;
    assert!(ThreadStore::new(Some(directory.path().into())).is_err());
    assert_eq!(
        crate::db::migration_version(&connection, "ThreadMetadataDb")?,
        999
    );
    let corrupt = tempfile::tempdir()?;
    std::fs::write(corrupt.path().join("threads.sqlite"), "not sqlite")?;
    assert!(ThreadStore::new(Some(corrupt.path().into())).is_err());
    assert_eq!(
        std::fs::read(corrupt.path().join("threads.sqlite"))?,
        b"not sqlite"
    );
    Ok(())
}
