use super::*;

fn thread(id: &str) -> ThreadRecord {
    ThreadRecord {
        thread_id: id.into(),
        session_id: Some(format!("remote-{id}")),
        agent_id: "test".into(),
        title: id.into(),
        updated_at: "2020-01-01".into(),
        created_at: Some("2019-01-01".into()),
        interacted_at: None,
        folder_paths: Some("[\"/repo\"]".into()),
        archived: false,
        title_override: Some("override".into()),
        parent: Some("{\"kind\":\"acp\",\"id\":\"parent\"}".into()),
        source: "cli".into(),
    }
}

#[test]
fn memory_handles_share_only_their_own_database_and_keep_opaque_snapshot_bytes() -> Result<()> {
    let db = AcpDb::open(None)?;
    let cloned = db.clone();
    let metadata = thread("a");
    // The storage layer must not interpret or normalize the caller's JSON encoding.
    let snapshot = b"{\n  \"version\": 1, \"future\": [1, 2]\n}";
    db.write(&[ThreadWrite::Upsert {
        metadata: &metadata,
        snapshot,
    }])?;
    assert_eq!(cloned.list_threads()?, vec![metadata]);
    assert_eq!(
        cloned.load_snapshot("a")?,
        Some(SnapshotRecord {
            data_type: "json".into(),
            data: snapshot.to_vec(),
        })
    );
    assert!(AcpDb::open(None)?.list_threads()?.is_empty());
    assert!(cloned.load_snapshot("missing")?.is_none());
    cloned.write(&[ThreadWrite::Delete("a")])?;
    assert!(db.list_threads()?.is_empty());
    assert!(db.load_snapshot("a")?.is_none());
    Ok(())
}

#[test]
fn a_failed_snapshot_rolls_back_the_entire_batch_and_can_be_retried() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("threads.sqlite");
    let db = AcpDb::open(Some(&path))?;
    let mut first = thread("a");
    let second = thread("b");
    db.write(&[ThreadWrite::Upsert {
        metadata: &first,
        snapshot: b"original",
    }])?;
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("sql/tests/reject_second_snapshot.sql"))?;
    first.title = "changed".into();
    let writes = [
        ThreadWrite::Upsert {
            metadata: &first,
            snapshot: b"changed",
        },
        ThreadWrite::Upsert {
            metadata: &second,
            snapshot: b"second",
        },
    ];
    assert!(db.write(&writes).is_err());
    assert_eq!(db.list_threads()?, vec![thread("a")]);
    assert_eq!(db.load_snapshot("a")?.unwrap().data, b"original");
    assert!(db.load_snapshot("b")?.is_none());
    connection.execute_batch(include_str!("sql/tests/allow_snapshot.sql"))?;
    db.write(&writes)?;
    drop(db);
    let restored = AcpDb::open(Some(&path))?;
    assert_eq!(restored.list_threads()?.len(), 2);
    assert_eq!(restored.load_snapshot("a")?.unwrap().data, b"changed");
    assert_eq!(restored.load_snapshot("b")?.unwrap().data, b"second");
    Ok(())
}

#[test]
fn a_failed_metadata_delete_restores_the_snapshot_and_earlier_batch_deletes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("threads.sqlite");
    let db = AcpDb::open(Some(&path))?;
    db.write(&[
        ThreadWrite::Upsert {
            metadata: &thread("a"),
            snapshot: b"first",
        },
        ThreadWrite::Upsert {
            metadata: &thread("b"),
            snapshot: b"second",
        },
    ])?;
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("sql/tests/reject_second_delete.sql"))?;
    assert!(
        db.write(&[ThreadWrite::Delete("a"), ThreadWrite::Delete("b")])
            .is_err()
    );
    assert_eq!(db.list_threads()?.len(), 2);
    assert_eq!(db.load_snapshot("a")?.unwrap().data, b"first");
    assert_eq!(db.load_snapshot("b")?.unwrap().data, b"second");
    Ok(())
}
