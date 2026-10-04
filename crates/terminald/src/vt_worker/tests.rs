use serde_json::json;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::Command,
    sync::{mpsc, watch},
    time::{Instant, timeout},
};

use super::{
    command::*, config::*, queue::*, rpc::*, session::*, snapshot::*, transport::*, worker::*,
};

fn test_client(capacity: usize) -> (VtWorkerClient, mpsc::Receiver<WorkerCommand>) {
    let (sender, receiver) = mpsc::channel(capacity);
    let (shutdown, _) = watch::channel(false);
    (
        VtWorkerClient {
            sender,
            shutdown: Arc::new(shutdown),
            scrollback: 100,
            sessions: Arc::new(Mutex::new(Vec::new())),
            cached_snapshot_bytes: Arc::new(AtomicUsize::new(0)),
            pending_write_bytes: Arc::new(AtomicUsize::new(0)),
        },
        receiver,
    )
}

fn cache_test_snapshot(state: &SessionState) {
    state.store_snapshot(
        0,
        VtSnapshot {
            generation: state.generation.clone(),
            applied_offset: 42,
            cols: 80,
            rows: 24,
            ansi: "old snapshot".to_owned(),
            lines: Vec::new(),
        },
    );
}

#[test]
fn snapshot_dimensions_are_defensively_bounded() {
    assert_eq!(result_dimension(&json!({ "cols": 1 }), "cols").unwrap(), 1);
    assert_eq!(
        result_dimension(&json!({ "rows": 1_000 }), "rows").unwrap(),
        1_000
    );
    for invalid in [0, 1_001, u16::MAX as u64] {
        assert!(result_dimension(&json!({ "cols": invalid }), "cols").is_err());
    }
}

#[tokio::test]
async fn full_queue_disables_only_the_affected_session_without_blocking() {
    let (client, mut receiver) = test_client(1);
    let first = client.create_session("first".to_owned(), 80, 24);
    assert!(first.state.is_active());
    let second = client.create_session("second".to_owned(), 80, 24);
    assert!(!second.state.is_active());
    assert!(first.state.is_active());
    assert!(matches!(
        receiver.recv().await,
        Some(WorkerCommand::Create { .. })
    ));
}

fn take_write(command: WorkerCommand) -> (Arc<SessionState>, WriteBatch) {
    let WorkerCommand::Write { session, batch } = command else {
        panic!("expected write command");
    };
    let write = batch.lock().unwrap().take().unwrap();
    (session, write)
}

#[tokio::test]
async fn interleaved_sessions_coalesce_output_without_filling_command_queue() {
    let (client, mut receiver) = test_client(2);
    let first = client.create_session("first".to_owned(), 80, 24);
    let second = client.create_session("second".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    receiver.recv().await.unwrap();

    // Includes UTF-8 and ANSI sequences split across individual writes.
    let first_output = "\u{1b}[31m你好\u{1b}[0m\r\n".as_bytes();
    let second_output = b"other terminal\r\n";
    for offset in 0..first_output.len().max(second_output.len()) {
        if let Some(byte) = first_output.get(offset) {
            first.write(offset as u64, &[*byte]);
        }
        if let Some(byte) = second_output.get(offset) {
            second.write(offset as u64, &[*byte]);
        }
    }
    assert!(first.state.is_active());
    assert!(second.state.is_active());
    assert_eq!(receiver.len(), 2, "one pending write per session");
    assert_eq!(
        client.pending_write_bytes.load(Ordering::Acquire),
        first_output.len() + second_output.len()
    );
    let (state, batch) = take_write(receiver.recv().await.unwrap());
    assert!(Arc::ptr_eq(&state, &first.state));
    assert_eq!(batch.start_offset, 0);
    assert_eq!(batch.data, first_output);
    drop(batch);
    let (state, batch) = take_write(receiver.recv().await.unwrap());
    assert!(Arc::ptr_eq(&state, &second.state));
    assert_eq!(batch.data, second_output);
    drop(batch);
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn consumed_batch_is_immutable_while_rpc_is_in_flight() {
    let (client, mut receiver) = test_client(2);
    let session = client.create_session("generation".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    session.write(0, b"first");
    let command = receiver.recv().await.unwrap();
    let WorkerCommand::Write { batch, .. } = &command else {
        panic!("expected write command");
    };
    let in_flight = batch.lock().unwrap().take().unwrap();
    session.write(5, b"second");
    assert_eq!(in_flight.data, b"first");
    let (_, next) = take_write(receiver.recv().await.unwrap());
    assert_eq!(next.start_offset, 5);
    assert_eq!(next.data, b"second");
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 11);
    drop((in_flight, next));
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_producer_and_consumer_preserve_every_byte() {
    let (client, mut receiver) = test_client(DEFAULT_QUEUE_CAPACITY);
    let session = client.create_session("generation".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    let expected: Vec<_> = (0..256 * 1024).map(|index| index as u8).collect();
    let output = expected.clone();
    let producer = tokio::task::spawn_blocking(move || {
        for (index, chunk) in output.chunks(31).enumerate() {
            session.write((index * 31) as u64, chunk);
        }
        assert!(session.state.is_active());
    });
    let mut restored = Vec::new();
    timeout(Duration::from_secs(5), async {
        while restored.len() < expected.len() {
            let (_, batch) = take_write(receiver.recv().await.unwrap());
            assert_eq!(batch.start_offset, restored.len() as u64);
            assert!(batch.data.len() <= MAX_WRITE_BATCH_BYTES);
            // Keep the consumed batch alive while the producer appends
            // more output, as happens while waiting for pipe capacity.
            tokio::task::yield_now().await;
            restored.extend_from_slice(&batch.data);
        }
    })
    .await
    .unwrap();
    producer.await.unwrap();
    assert_eq!(restored, expected);
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn coalescing_preserves_resize_dispose_and_offset_boundaries() {
    let (client, mut receiver) = test_client(8);
    let session = client.create_session("generation".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    session.write(0, b"before");
    session.resize(6, 100, 30);
    session.write(6, b"after");
    // Leave gaps/duplicates intact so the existing worker offset validation
    // can reject them instead of silently accepting merged corrupt output.
    session.write(20, b"gap");
    session.write(20, b"duplicate");
    session.dispose();
    session.write(29, b"ignored");

    let (_, before) = take_write(receiver.recv().await.unwrap());
    assert_eq!((before.revision, before.start_offset), (0, 0));
    assert_eq!(before.data, b"before");
    assert!(matches!(
        receiver.recv().await.unwrap(),
        WorkerCommand::Resize {
            revision: 1,
            at_offset: 6,
            cols: 100,
            rows: 30,
            ..
        }
    ));
    let (_, after) = take_write(receiver.recv().await.unwrap());
    assert_eq!((after.revision, after.start_offset), (1, 6));
    assert_eq!(after.data, b"after");
    let (_, gap) = take_write(receiver.recv().await.unwrap());
    assert_eq!((gap.revision, gap.start_offset), (1, 20));
    assert_eq!(gap.data, b"gap");
    let (_, duplicate) = take_write(receiver.recv().await.unwrap());
    assert_eq!(duplicate.start_offset, 20);
    assert_eq!(duplicate.data, b"duplicate");
    assert!(matches!(
        receiver.recv().await.unwrap(),
        WorkerCommand::Dispose { .. }
    ));
    assert!(receiver.is_empty());
    drop((before, after, gap, duplicate));
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn batches_bound_bytes_and_release_budget_when_commands_are_dropped() {
    let (client, mut receiver) = test_client(4);
    let session = client.create_session("generation".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    let output = vec![b'x'; MAX_WRITE_BATCH_BYTES * 2 + 1];
    session.write(0, &output);
    assert_eq!(receiver.len(), 3);
    for offset in [0, MAX_WRITE_BATCH_BYTES, MAX_WRITE_BATCH_BYTES * 2] {
        let (_, batch) = take_write(receiver.recv().await.unwrap());
        assert_eq!(batch.start_offset, offset as u64);
        assert!(batch.data.len() <= MAX_WRITE_BATCH_BYTES);
        assert_eq!(
            batch.data,
            output[offset..(offset + MAX_WRITE_BATCH_BYTES).min(output.len())]
        );
    }
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
    session.write(output.len() as u64, b"queued");
    drop(receiver);
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn shared_write_byte_budget_disables_only_the_session_that_exceeds_it() {
    let (client, mut receiver) = test_client(DEFAULT_QUEUE_CAPACITY);
    let first = client.create_session("first".to_owned(), 80, 24);
    let second = client.create_session("second".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    receiver.recv().await.unwrap();
    let data = vec![b'x'; MAX_WRITE_BATCH_BYTES];
    for offset in (0..MAX_PENDING_WRITE_BYTES).step_by(data.len()) {
        first.write(offset as u64, &data);
    }
    assert!(first.state.is_active());
    assert_eq!(
        client.pending_write_bytes.load(Ordering::Acquire),
        MAX_PENDING_WRITE_BYTES
    );
    second.write(0, b"over budget");
    assert!(!second.state.is_active());
    assert!(first.state.is_active());
    drop(receiver);
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn rejected_write_command_releases_its_byte_budget() {
    let (client, mut receiver) = test_client(1);
    let session = client.create_session("generation".to_owned(), 80, 24);
    receiver.recv().await.unwrap();
    session.write(0, b"first");
    session.resize(5, 100, 30);
    assert!(!session.state.is_active());
    drop(receiver);
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);

    let (client, _receiver) = test_client(1);
    let session = client.create_session("generation".to_owned(), 80, 24);
    session.write(0, b"rejected behind create");
    assert!(!session.state.is_active());
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn resize_synchronously_invalidates_old_snapshot() {
    let (client, mut receiver) = test_client(4);
    let session = client.create_session("generation".to_owned(), 80, 24);
    assert!(matches!(
        receiver.recv().await,
        Some(WorkerCommand::Create { .. })
    ));
    cache_test_snapshot(&session.state);
    assert!(session.snapshot().is_some());

    session.resize(42, 120, 40);
    assert_eq!(
        session.snapshot(),
        None,
        "old-size state must be unavailable before the resize RPC runs"
    );
    assert!(matches!(
        receiver.recv().await,
        Some(WorkerCommand::Resize {
            revision: 1,
            at_offset: 42,
            cols: 120,
            rows: 40,
            ..
        })
    ));
}

#[test]
fn worker_rejection_clears_snapshot_and_disables_session() {
    let (client, _receiver) = test_client(4);
    let session = client.create_session("generation".to_owned(), 80, 24);
    cache_test_snapshot(&session.state);
    assert!(session.snapshot().is_some());

    let fatal = handle_command_error(
        &session.state,
        &RpcError::Worker("offset_mismatch".to_owned()),
    );
    assert!(
        fatal.is_none(),
        "a session rejection need not kill the worker"
    );
    assert!(!session.state.is_active());
    assert!(session.snapshot().is_none());
}

#[tokio::test]
async fn create_write_and_resize_only_schedule_snapshot_work() {
    let (client, mut receiver) = test_client(8);
    let session = client.create_session("generation".to_owned(), 80, 24);
    let WorkerCommand::Create { session: state, .. } = receiver.recv().await.unwrap() else {
        panic!("expected create command");
    };
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = RpcClient::new(
        child.stdin.take().unwrap(),
        child.stdout.take().unwrap(),
        Duration::from_secs(5),
        client.sessions.clone(),
    );

    assert!(
        process_command(
            WorkerCommand::Create {
                session: state.clone(),
                cols: 80,
                rows: 24
            },
            &mut rpc
        )
        .await
        .unwrap()
    );
    assert!(state.snapshot.lock().unwrap().is_none());

    session.write(0, &vec![b'x'; SNAPSHOT_BYTE_INTERVAL]);
    while let Ok(command) = receiver.try_recv() {
        let urgent = process_command(command, &mut rpc).await.unwrap();
        assert_eq!(
            urgent,
            state.dirty_bytes.load(Ordering::Acquire) >= SNAPSHOT_BYTE_INTERVAL
        );
    }
    assert!(state.dirty_bytes.load(Ordering::Acquire) >= SNAPSHOT_BYTE_INTERVAL);
    assert!(state.snapshot.lock().unwrap().is_none());

    let revision = state.invalidate_for_resize().unwrap();
    assert!(
        process_command(
            WorkerCommand::Resize {
                session: state.clone(),
                revision,
                at_offset: SNAPSHOT_BYTE_INTERVAL as u64,
                cols: 120,
                rows: 40,
            },
            &mut rpc
        )
        .await
        .unwrap()
    );
    assert!(state.snapshot.lock().unwrap().is_none());
    assert_ne!(state.dirty_bytes.load(Ordering::Acquire), 0);
    assert!(
        !process_command(
            WorkerCommand::Dispose {
                session: state.clone(),
            },
            &mut rpc
        )
        .await
        .unwrap()
    );
    let _ = child.start_kill();
    let _ = child.wait().await;
    drop(session);
}

#[tokio::test]
async fn real_worker_tracks_offsets_and_resize_geometry() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    if !script.exists() {
        eprintln!("skipping real VT worker test because the bundle is unavailable");
        return;
    }
    let worker = VtWorker::start(VtWorkerConfig::new("node", script))
        .await
        .expect("real VT worker starts");
    let session = worker
        .client()
        .create_session("test-epoch".to_owned(), 80, 24);

    wait_for_snapshot(&session, |snapshot| snapshot.applied_offset == 0).await;
    session.write(0, b"hello\r\nworld");
    let written = wait_for_snapshot(&session, |snapshot| snapshot.applied_offset == 12).await;
    assert_eq!((written.cols, written.rows), (80, 24));
    assert!(written.ansi.contains("hello"));

    session.resize(12, 120, 40);
    assert!(session.snapshot().is_none());
    let resized = wait_for_snapshot(&session, |snapshot| {
        snapshot.applied_offset == 12 && snapshot.cols == 120 && snapshot.rows == 40
    })
    .await;
    assert!(resized.ansi.contains("world"));
    session.dispose();
    worker.shutdown().await;
}

#[tokio::test]
async fn real_worker_preserves_snapshots_across_one_column_create_and_resize() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    let worker = VtWorker::start(VtWorkerConfig::new("node", script))
        .await
        .expect("real VT worker starts");
    for initial_cols in [1, 80] {
        let session =
            worker
                .client()
                .create_session(format!("one-column-{initial_cols}"), initial_cols, 1);
        let initial = wait_for_snapshot(&session, |_| true).await;
        assert_eq!((initial.cols, initial.rows), (initial_cols.max(2), 1));

        let mut offset = 0;
        for (cols, text) in [(1, "好"), (80, "restored")] {
            session.resize(offset, cols, 1);
            assert!(session.snapshot().is_none());
            let data = format!("\r\x1b[2K{text}");
            session.write(offset, data.as_bytes());
            offset += data.len() as u64;
            let snapshot =
                wait_for_snapshot(&session, |snapshot| snapshot.applied_offset == offset).await;
            assert!(session.state.is_active());
            assert_eq!((snapshot.cols, snapshot.rows), (cols.max(2), 1));
            assert_eq!(snapshot.lines, vec![text.to_owned()]);
        }
        session.dispose();
    }
    worker.shutdown().await;
}

#[tokio::test]
async fn mutation_errors_invalidate_only_the_matching_session_without_waiting_for_a_query() {
    let (client, mut receiver) = test_client(8);
    let session = client.create_session("generation".to_owned(), 80, 24);
    let other = client.create_session("other".to_owned(), 80, 24);
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = RpcClient::new(
        child.stdin.take().unwrap(),
        child.stdout.take().unwrap(),
        Duration::from_secs(5),
        client.sessions.clone(),
    );
    process_command(receiver.recv().await.unwrap(), &mut rpc)
        .await
        .unwrap();
    process_command(receiver.recv().await.unwrap(), &mut rpc)
        .await
        .unwrap();
    cache_test_snapshot(&session.state);
    cache_test_snapshot(&other.state);

    // A delayed error for a stale generation must not disable this one.
    rpc.send(
        "write",
        json!({ "session_id": session.state.session_id,
            "generation": "stale", "start_offset": 0 }),
        b"stale",
    )
    .await
    .unwrap();
    // This query is a barrier after the stale error, not a write ACK.
    rpc.call(
        "snapshot",
        json!({ "session_id": session.state.session_id,
            "generation": session.state.generation }),
    )
    .await
    .unwrap();
    assert!(session.state.is_active());

    rpc.send(
        "write",
        json!({ "session_id": session.state.session_id,
            "generation": session.state.generation, "start_offset": 999 }),
        b"gap",
    )
    .await
    .unwrap();
    // No call/receive here: the independent reader must apply the error.
    timeout(Duration::from_secs(5), async {
        while session.state.is_active() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(session.snapshot().is_none());
    assert!(other.state.is_active());
    assert!(other.snapshot().is_some());
    drop(rpc);
    child.start_kill().unwrap();
    child.wait().await.unwrap();
}

#[tokio::test]
async fn worker_exit_invalidates_cached_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("exit-worker.mjs");
    let framing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/src/framing.mjs");
    let source = format!(
        r#"
import {{ encodeFrame, readFrames }} from {};
for await (const frame of readFrames(process.stdin)) {{
  const request = JSON.parse(frame.metadata);
  if (request.action === 'write') process.exit(0);
  const result = {{ session_id: request.session_id, generation: request.generation,
    applied_offset: 0, cols: 80, rows: 24, lines: [], byte_length: 0 }};
  process.stdout.write(encodeFrame({{ id: request.id, action: request.action, ok: true, result }}));
}}
"#,
        serde_json::to_string(&framing).unwrap()
    );
    std::fs::write(&script, source).unwrap();
    let worker = VtWorker::start(VtWorkerConfig::new("node", script))
        .await
        .unwrap();
    let session = worker.client().create_session("exiting".to_owned(), 80, 24);
    wait_for_snapshot(&session, |_| true).await;
    session.write(0, b"exit now");
    timeout(Duration::from_secs(5), async {
        while session.state.is_active() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(session.snapshot().is_none());
    worker.shutdown().await;
}

#[tokio::test]
async fn blocked_worker_pipe_does_not_block_producers_or_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("blocked-worker.mjs");
    let framing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/src/framing.mjs");
    let source = format!(
        r#"
import {{ encodeFrame, readFrames }} from {};
// Stop reading after the first write, without acknowledging it or exiting.
for await (const frame of readFrames(process.stdin)) {{
  const request = JSON.parse(frame.metadata);
  if (request.action === 'write') {{
    setInterval(() => {{}}, 1000);
    await new Promise(() => {{}});
  }}
  process.stdout.write(encodeFrame({{ id: request.id, action: request.action, ok: true,
    result: {{ session_id: request.session_id, generation: request.generation,
      applied_offset: 0, cols: 80, rows: 24, lines: [], byte_length: 0 }} }}));
}}
"#,
        serde_json::to_string(&framing).unwrap()
    );
    std::fs::write(&script, source).unwrap();
    let worker = VtWorker::start(VtWorkerConfig::new("node", script))
        .await
        .unwrap();
    let client = worker.client();
    let session = client.create_session("blocked".to_owned(), 80, 24);
    wait_for_snapshot(&session, |_| true).await;
    let producer = tokio::task::spawn_blocking(move || {
        let data = vec![b'x'; MAX_WRITE_BATCH_BYTES];
        for index in 0..1024 {
            session.write((index * data.len()) as u64, &data);
        }
        assert!(
            !session.state.is_active(),
            "bounded queue must degrade a stalled session"
        );
        session
    });
    let session = timeout(Duration::from_secs(3), producer)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(3), worker.shutdown())
        .await
        .expect("shutdown cancels pipe/RPC waits");
    assert!(session.snapshot().is_none());
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn snapshots_must_confirm_the_sent_offset_and_geometry() {
    for bad_offset in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("bad-snapshot.mjs");
        let framing =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/src/framing.mjs");
        let source = format!(
            r#"
import {{ encodeFrame, readFrames }} from {};
for await (const frame of readFrames(process.stdin)) {{
  const request = JSON.parse(frame.metadata);
  if (request.action === 'write' || request.action === 'resize') continue;
  process.stdout.write(encodeFrame({{ id: request.id, action: request.action, ok: true,
    result: {{ session_id: request.session_id, generation: request.generation,
      applied_offset: 0, cols: 80, rows: 24, lines: [], byte_length: 0 }} }}));
}}
"#,
            serde_json::to_string(&framing).unwrap()
        );
        std::fs::write(&script, source).unwrap();
        let worker = VtWorker::start(VtWorkerConfig::new("node", script))
            .await
            .unwrap();
        let session = worker
            .client()
            .create_session("bad-snapshot".to_owned(), 80, 24);
        wait_for_snapshot(&session, |_| true).await;
        if bad_offset {
            session.write(0, b"missing");
        } else {
            session.resize(0, 120, 40);
        }
        timeout(Duration::from_secs(5), async {
            while session.state.is_active() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(session.snapshot().is_none());
        worker.shutdown().await;
    }
}

#[tokio::test]
async fn real_worker_restores_interleaved_small_write_bursts() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    let worker = VtWorker::start(VtWorkerConfig::new("node", script))
        .await
        .unwrap();
    let client = worker.client();
    let sessions: Vec<_> = (0..8)
        .map(|index| client.create_session(format!("burst-{index}"), 120, 35))
        .collect();
    let mut outputs = Vec::new();
    for index in 0..sessions.len() {
        let mut output = vec![b'x'; 256 * 1024];
        let marker = format!("\u{1b}[?1049h\u{1b}[2J\u{1b}[H你好 pane={index}");
        let start = output.len() - marker.len();
        output[start..].copy_from_slice(marker.as_bytes());
        outputs.push(output);
    }
    let started = Instant::now();
    // No yield: the old per-read queue overflowed before the actor could
    // consume 8,192 interleaved messages. Batching uses just 32 write slots.
    for offset in (0..outputs[0].len()).step_by(256) {
        for (session, output) in sessions.iter().zip(&outputs) {
            session.write(offset as u64, &output[offset..offset + 256]);
        }
    }
    assert!(sessions.iter().all(|session| session.state.is_active()));
    for (index, session) in sessions.iter().enumerate() {
        let snapshot = wait_for_snapshot(session, |snapshot| {
            snapshot.applied_offset == outputs[index].len() as u64
        })
        .await;
        assert_eq!((snapshot.cols, snapshot.rows), (120, 35));
        assert_eq!(snapshot.lines[0], format!("你好 pane={index}"));
        assert!(snapshot.ansi.contains("\u{1b}[?1049h"));
    }
    eprintln!(
        "8 sessions, 2 MiB in 8,192 small writes restored in {:?}",
        started.elapsed()
    );
    assert_eq!(client.pending_write_bytes.load(Ordering::Acquire), 0);
    for session in sessions {
        session.dispose();
    }
    worker.shutdown().await;
}

async fn wait_for_snapshot(
    session: &VtSession,
    predicate: impl Fn(&VtSnapshot) -> bool,
) -> VtSnapshot {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(snapshot) = session.snapshot()
                && predicate(&snapshot)
            {
                break snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("VT snapshot appeared before timeout")
}
