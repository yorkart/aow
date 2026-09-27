use super::*;
use std::{fs, io::Write};

fn record(index: usize) -> Record {
    Record {
        timestamp: "2026-09-23T12:00:00Z".into(),
        operation_id: format!("op-{index}"),
        boot_id: "boot".into(),
        kind: "worktree.remove".into(),
        source: "web".into(),
        title: "删除 Worktree".into(),
        event: "finished".into(),
        level: Level::Info,
        message: format!("结果 {index}\n包含多行和中文"),
        project_id: Some("project".into()),
        resource: None,
        outcome: Some(Outcome::Succeeded),
        completed: None,
        total: None,
    }
}

fn write_records(path: &Path, indices: impl Iterator<Item = usize>) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for index in indices {
        serde_json::to_writer(&mut file, &record(index)).unwrap();
        file.write_all(b"\n").unwrap();
    }
}

#[test]
fn rolling_writer_roundtrips_structured_events() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path(), 48).unwrap();
    writer.append(&record(1)).unwrap();
    let page = Reader::new(dir.path())
        .read(&ReadOptions::default())
        .unwrap();
    assert_eq!(page.items, vec![record(1)]);
    assert!(page.next_cursor.is_none());
}

#[test]
fn cursor_crosses_files_without_duplicates_when_new_logs_arrive() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("operations.2026-09-23-11.log");
    let new = dir.path().join("operations.2026-09-23-12.log");
    write_records(&old, 0..3);
    write_records(&new, 3..7);
    let reader = Reader::new(dir.path());
    let mut options = ReadOptions {
        limit: 2,
        ..Default::default()
    };
    let first = reader.read(&options).unwrap();
    assert_eq!(first.items, vec![record(6), record(5)]);
    write_records(&new, 7..9);
    write_records(&dir.path().join("operations.2026-09-23-13.log"), 9..10);
    options.cursor = first.next_cursor;
    let mut found = first.items;
    while options.cursor.is_some() {
        let page = reader.read(&options).unwrap();
        options.cursor = page.next_cursor;
        found.extend(page.items);
    }
    assert_eq!(found, (0..7).rev().map(record).collect::<Vec<_>>());
    assert_eq!(
        reader.read(&ReadOptions::default()).unwrap().items[0],
        record(9)
    );
}

#[test]
fn sparse_filter_makes_progress_with_empty_budget_limited_pages() {
    let dir = tempfile::tempdir().unwrap();
    write_records(&dir.path().join("operations.2026-09-23-12.log"), 0..2000);
    let reader = Reader::new(dir.path());
    let mut options = ReadOptions {
        scan_bytes: MIN_SCAN_BYTES,
        filter: Filter {
            operation_id: Some("op-1".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut found = Vec::new();
    let mut empty_pages = 0;
    for _ in 0..100 {
        let page = reader.read(&options).unwrap();
        assert!(page.scanned_bytes <= options.scan_bytes);
        if page.budget_exhausted && page.items.is_empty() {
            empty_pages += 1;
        }
        found.extend(page.items);
        let Some(cursor) = page.next_cursor else {
            break;
        };
        if let Some(previous) = &options.cursor {
            assert!(cursor.offset < previous.offset);
        }
        options.cursor = Some(cursor);
    }
    assert!(empty_pages > 0);
    assert_eq!(found, vec![record(1)]);
}

#[test]
fn chunk_boundaries_torn_tails_and_oversized_lines_remain_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("operations.2026-09-23-12.log");
    write_records(&path, 0..150);
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&vec![b'x'; MAX_RECORD_BYTES * 4]).unwrap();
    file.write_all(b"\ninvalid json\n").unwrap();
    write_records(&path, 150..200);
    file.write_all(&serde_json::to_vec(&record(200)).unwrap())
        .unwrap(); // no newline
    let reader = Reader::new(dir.path());
    let mut options = ReadOptions {
        limit: 17,
        scan_bytes: MIN_SCAN_BYTES,
        ..Default::default()
    };
    let mut found = Vec::new();
    for _ in 0..100 {
        let page = reader.read(&options).unwrap();
        assert!(page.items.len() <= 17);
        assert!(page.scanned_bytes <= MIN_SCAN_BYTES);
        found.extend(page.items);
        let Some(cursor) = page.next_cursor else {
            break;
        };
        assert_ne!(options.cursor.as_ref(), Some(&cursor));
        options.cursor = Some(cursor);
    }
    assert_eq!(found, (0..200).rev().map(record).collect::<Vec<_>>());
    file.write_all(b"\n").unwrap();
    assert_eq!(
        reader.read(&ReadOptions::default()).unwrap().items[0],
        record(200)
    );
}

#[test]
fn cursors_reject_paths_missing_files_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let reader = Reader::new(dir.path());
    let mut options = ReadOptions {
        cursor: Some(Cursor {
            file: "../secret".into(),
            offset: 1,
        }),
        ..Default::default()
    };
    assert!(matches!(reader.read(&options), Err(Error::InvalidCursor)));
    options.cursor.as_mut().unwrap().file = "operations.2026-09-23-12.log".into();
    assert!(matches!(reader.read(&options), Err(Error::CursorExpired)));
    fs::write(dir.path().join(&options.cursor.as_ref().unwrap().file), "").unwrap();
    assert!(matches!(reader.read(&options), Err(Error::CursorExpired)));
}

#[test]
fn writer_separates_a_torn_record_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path(), 48).unwrap();
    writer.append(&record(0)).unwrap();
    drop(writer);
    let path = fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"{\"torn\":")
        .unwrap();
    let writer = Writer::open(dir.path(), 48).unwrap();
    writer.append(&record(1)).unwrap();
    assert_eq!(
        Reader::new(dir.path())
            .read(&ReadOptions::default())
            .unwrap()
            .items,
        vec![record(1), record(0)]
    );
}

#[test]
fn large_files_only_read_a_page_or_the_scan_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("operations.2026-09-23-12.log");
    let mut file = fs::File::create(&path).unwrap();
    file.set_len(512 * 1024 * 1024).unwrap(); // sparse, without allocating 512 MiB
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(b"\n").unwrap();
    write_records(&path, 0..100);
    let reader = Reader::new(dir.path());
    let page = reader
        .read(&ReadOptions {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.items, (90..100).rev().map(record).collect::<Vec<_>>());
    assert!(page.scanned_bytes <= 16 * 1024);
    let page = reader
        .read(&ReadOptions {
            filter: Filter {
                query: Some("not present".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(page.items.is_empty());
    assert!(page.budget_exhausted);
    assert_eq!(page.scanned_bytes, DEFAULT_SCAN_BYTES);
    assert!(page.next_cursor.unwrap().offset < file.metadata().unwrap().len());
}
