use super::*;
use crate::{
    acp_thread::{AcpThread, history::ThreadStore},
    facade::{Permission, ThreadEntry},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
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
        message_id: Some("protocol-reply".into()),
        delivery: None,
        content: json!({"type":"text","text":"original"}),
    }];
    thread
}
fn upsert(snapshot: SessionSnapshot) -> DbOperation {
    DbOperation::Upsert(
        Box::new(ThreadMetadata::from_snapshot(&snapshot, None)),
        Arc::new(Mutex::new(snapshot)),
    )
}
fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(
            &format!(include_str!("sql/tests/count_rows.sql"), table = table),
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn load_snapshot(connection: &Connection, id: &str) -> Result<Option<SessionSnapshot>> {
    let row = connection
        .query_row(include_str!("sql/tests/load_snapshot.sql"), [id], |row| {
            Ok(aow_db::acp::SnapshotRecord {
                data_type: row.get(0)?,
                data: row.get(1)?,
            })
        })
        .optional()?;
    row.map(|row| snapshot_store::decode(id, row)).transpose()
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
    assert!(operations.iter().any(|op| matches!(op, DbOperation::Upsert(metadata, snapshot) if metadata.thread_id == "a" && metadata.title == "Latest" && snapshot.lock().unwrap().title == "Latest")));
    assert!(operations.iter().any(|op| op.id() == "b"));
}

#[test]
fn sqlite_round_trip_keeps_metadata_and_content_without_json_rewrites() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    let mut snapshot = thread("a");
    for kind in ["user", "user", "assistant", "assistant"] {
        AcpThread::push(
            &mut snapshot,
            kind,
            json!({"type":"text","text":"separate content block"}),
        );
    }
    AcpThread::handle_session_update(
        &mut snapshot,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"completed","summary":[{"type":"text","text":"Summary"}]}),
    );
    snapshot.plan = json!({"entries":[{"content":"task","status":"in_progress"}]});
    snapshot.notices.push(ThreadEntry {
        id: "notice".into(),
        kind: "notice".into(),
        message_id: None,
        delivery: None,
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
    let journal: String =
        connection.query_row(include_str!("sql/tests/journal_mode.sql"), [], |row| {
            row.get(0)
        })?;
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
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
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
    assert_eq!(actual.interacted_at, expected.interacted_at);
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
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    store.insert(thread("good"))?;
    store.insert(thread("bad"))?;
    {
        let connection = Connection::open(directory.path().join("threads.sqlite"))?;
        connection.execute(include_str!("sql/tests/corrupt_snapshot.sql"), [])?;
    }
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
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
fn json_history_is_ignored_and_left_untouched() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let history = directory.path().join("history");
    std::fs::create_dir(&history)?;
    let original = serde_json::to_vec(&thread("old"))?;
    std::fs::write(history.join("old.json"), &original)?;
    std::fs::write(history.join("corrupt.json"), "{truncated")?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    assert!(store.sessions(None).is_empty());
    store.insert(thread("current"))?;
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
    assert_eq!(restored.sessions(None).len(), 1);
    assert_eq!(
        restored.snapshot("current")?.entries[0].content["text"],
        "original"
    );
    assert!(restored.snapshot("old").is_err());
    assert_eq!(std::fs::read(history.join("old.json"))?, original);
    assert_eq!(std::fs::read(history.join("corrupt.json"))?, b"{truncated");
    Ok(())
}

#[test]
fn streaming_saves_coalesce_and_turn_end_flushes_latest_content() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    let mut connection = Connection::open(directory.path().join("threads.sqlite"))?;
    connection.execute_batch(include_str!("sql/tests/count_snapshot_updates.sql"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Hold SQLite while notifications arrive: the pending queue must retain only the latest state.
    for index in 0..200 {
        store.update("a", |thread| {
            thread.entries[0].content["text"] = format!("chunk-{index}").into()
        })?;
    }
    drop(transaction);
    store.update("a", |thread| thread.status = "idle".into())?;
    let saved = load_snapshot(&connection, "a")?.unwrap();
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
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    store.update("a", |thread| {
        thread.entries[0].content["text"] = "checkpoint".into()
    })?;
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if load_snapshot(&connection, "a")?.unwrap().entries[0].content["text"] == "checkpoint" {
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
        load_snapshot(&connection, "a")?.unwrap().entries[0].content["text"],
        "at shutdown"
    );
    Ok(())
}

#[test]
fn database_failure_rolls_back_metadata_and_body_and_retry_keeps_latest_state() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    store.insert(thread("a"))?;
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    connection.execute_batch(include_str!("sql/tests/reject_snapshot.sql"))?;
    assert!(
        store
            .update("a", |thread| {
                thread.title = "failed".into();
                thread.entries[0].content["text"] = "failed".into();
            })
            .is_err()
    );
    {
        assert_eq!(
            connection.query_row(include_str!("sql/tests/load_title.sql"), [], |row| {
                row.get::<_, String>(0)
            })?,
            "Conversation a"
        );
        assert_eq!(
            load_snapshot(&connection, "a")?.unwrap().entries[0].content["text"],
            "original"
        );
        connection.execute_batch(include_str!("sql/tests/allow_snapshot.sql"))?;
    }
    store.update("a", |thread| {
        thread.title = "latest".into();
        thread.entries[0].content["text"] = "latest".into();
    })?;
    assert_eq!(load_snapshot(&connection, "a")?.unwrap().title, "latest");
    Ok(())
}

#[test]
fn queued_updates_cannot_resurrect_a_deleted_thread() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    store.insert(thread("a"))?;
    store.update("a", |thread| thread.status = "working".into())?;
    let mut connection = Connection::open(directory.path().join("threads.sqlite"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for index in 0..50 {
        store.update("a", |thread| thread.title = format!("update {index}"))?;
    }
    drop(transaction);
    store.remove("a")?;
    store.metadata.flush()?;
    assert!(store.sessions(None).is_empty());
    assert_eq!(count(&connection, "sidebar_threads"), 0);
    assert_eq!(count(&connection, "acp_thread_snapshots"), 0);
    drop(connection);
    assert!(
        ThreadStore::new(Some(directory.path().into()), || {})?
            .sessions(None)
            .is_empty()
    );
    Ok(())
}

#[test]
fn future_schema_and_corrupt_database_fail_without_silent_reset() -> Result<()> {
    let directory = tempfile::tempdir()?;
    drop(ThreadStore::new(Some(directory.path().into()), || {})?);
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    connection.execute(include_str!("sql/tests/future_schema.sql"), [])?;
    assert!(ThreadStore::new(Some(directory.path().into()), || {}).is_err());
    assert_eq!(
        connection.query_row(
            include_str!("sql/tests/migration_version.sql"),
            ["ThreadMetadataDb"],
            |row| row.get::<_, usize>(0)
        )?,
        999
    );
    let corrupt = tempfile::tempdir()?;
    std::fs::write(corrupt.path().join("threads.sqlite"), "not sqlite")?;
    assert!(ThreadStore::new(Some(corrupt.path().into()), || {}).is_err());
    assert_eq!(
        std::fs::read(corrupt.path().join("threads.sqlite"))?,
        b"not sqlite"
    );
    Ok(())
}

#[test]
fn origin_and_creation_time_survive_updates_restart_and_missing_snapshot() -> Result<()> {
    use crate::facade::SessionSource;
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    let mut cli = thread("cli-root");
    cli.source = SessionSource::Cli;
    cli.created_at = "2020-01-01T00:00:00Z".into();
    store.insert(cli)?;
    let mut user = thread("user-child");
    user.parent = Some(aow_protocol::InstanceRef::acp("cli-root"));
    store.insert(user)?;
    store.update("cli-root", |snapshot| snapshot.title = "changed".into())?;
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
    let info = restored.session_info("cli-root")?;
    assert_eq!(info.source, SessionSource::Cli);
    assert_eq!(info.created_at, "2020-01-01T00:00:00Z");
    assert!(info.parent.is_none());
    assert_eq!(
        restored.session_info("user-child")?.source,
        SessionSource::User
    );
    assert!(restored.threads.is_empty());
    assert_eq!(restored.snapshot("cli-root")?.created_at, info.created_at);
    Connection::open(directory.path().join("threads.sqlite"))?
        .execute(include_str!("sql/tests/delete_cli_snapshot.sql"), [])?;
    drop(restored);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
    let snapshot = restored.snapshot("cli-root")?;
    assert_eq!(snapshot.source, SessionSource::Cli);
    assert_eq!(snapshot.created_at, info.created_at);
    assert!(snapshot.needs_load);
    Ok(())
}

#[test]
fn origin_migration_defaults_old_records_to_user_without_inferring_from_parent() -> Result<()> {
    use crate::facade::SessionSource;
    let directory = tempfile::tempdir()?;
    let connection = Connection::open(directory.path().join("threads.sqlite"))?;
    connection.execute_batch(include_str!("sql/tests/legacy_database.sql"))?;
    connection.execute(include_str!("sql/tests/insert_legacy_thread.sql"), [])?;
    // Legacy snapshots lack newer fields; hydration restores persisted metadata.
    let mut snapshot = thread("old");
    snapshot.session_id = None;
    connection.execute(
        include_str!("sql/tests/save_snapshot.sql"),
        rusqlite::params![snapshot.thread_id, snapshot_store::encode(&snapshot)?],
    )?;
    connection.execute(
        include_str!("sql/tests/remove_legacy_snapshot_fields.sql"),
        [],
    )?;
    drop(connection);
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    assert_eq!(store.session_info("old")?.source, SessionSource::User);
    assert_eq!(store.session_info("old")?.created_at, "2020-01-01");
    assert_eq!(store.snapshot("old")?.source, SessionSource::User);
    assert_eq!(store.snapshot("old")?.created_at, "2020-01-01");
    assert_eq!(
        store.snapshot("old")?.interacted_at.as_deref(),
        Some("2020-02-01")
    );
    store.update("old", |thread| thread.status = "idle".into())?;
    assert_eq!(
        store.session_info("old")?.interacted_at.as_deref(),
        Some("2020-02-01")
    );
    drop(store);
    let store = ThreadStore::new(Some(directory.path().into()), || {})?;
    assert_eq!(
        store.session_info("old")?.interacted_at.as_deref(),
        Some("2020-02-01")
    );
    Ok(())
}
