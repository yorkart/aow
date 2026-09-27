use std::{
    future::Future,
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU8, AtomicU16, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::{
    process::{Child, Command},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{Instant, sleep_until, timeout},
};
use uuid::Uuid;

mod transport;
use transport::RpcClient;

const DEFAULT_QUEUE_CAPACITY: usize = 2_048;
const MAX_WRITE_BATCH_BYTES: usize = 64 * 1024;
// Previously each queue entry held at most one 8 KiB PTY read. Keep that
// aggregate byte bound when multiple reads can share a queued command.
// A sent batch releases its reservation once the pipe accepts it. The worker
// reads/executes one frame at a time, so pipe backpressure bounds the rest.
const MAX_PENDING_WRITE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SCROLLBACK: usize = 100;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(200);
// Keep this aligned with vt-worker/src/vt-service.mjs.
const MAX_SCROLLBACK: usize = 100;
// Keep this aligned with vt-worker/src/protocol.mjs.
const MAX_CACHED_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_CACHED_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const SNAPSHOT_BYTE_INTERVAL: usize = 256 * 1024;
// Snapshot work shares one ordered actor with write and resize RPCs. Bound
// each timer pass so a large set of dirty sessions cannot monopolize the
// worker or fill the non-blocking command queue. The cursor below makes the
// bounded pass fair across ticks.
const MAX_SNAPSHOTS_PER_TICK: usize = 4;
const SNAPSHOT_TICK_BUDGET: Duration = Duration::from_millis(50);
// A byte-threshold or resize wakeup may bring the normal 200ms deadline
// forward, but it cannot create an unbounded serialize-per-command loop.
const MIN_SNAPSHOT_PASS_SPACING: Duration = Duration::from_millis(50);
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

const SESSION_ACTIVE: u8 = 0;
const SESSION_DISPOSING: u8 = 1;
const SESSION_DISPOSED: u8 = 2;
const SESSION_FAILED: u8 = 3;

/// Configuration for the optional headless-xterm sidecar.
///
/// `program` is normally a Node executable. `script` is either terminald's
/// extracted embedded bundle or an explicit development override.
#[derive(Clone, Debug)]
pub struct VtWorkerConfig {
    pub program: PathBuf,
    pub script: PathBuf,
    pub queue_capacity: usize,
    pub request_timeout: Duration,
    pub snapshot_interval: Duration,
    pub scrollback: usize,
}

impl VtWorkerConfig {
    pub fn new(program: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            script: script.into(),
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            snapshot_interval: DEFAULT_SNAPSHOT_INTERVAL,
            scrollback: DEFAULT_SCROLLBACK,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VtSnapshot {
    pub(crate) generation: String,
    pub(crate) applied_offset: u64,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) ansi: String,
    pub(crate) lines: Vec<String>,
}

pub(crate) struct VtWorker {
    client: VtWorkerClient,
    task: Option<JoinHandle<()>>,
}

impl VtSnapshot {
    fn cached_bytes(&self) -> usize {
        self.ansi.len() + self.lines.iter().map(String::len).sum::<usize>()
    }
}

#[derive(Clone)]
pub(crate) struct VtWorkerClient {
    sender: mpsc::Sender<WorkerCommand>,
    shutdown: Arc<watch::Sender<bool>>,
    scrollback: usize,
    sessions: Arc<Mutex<Vec<Weak<SessionState>>>>,
    cached_snapshot_bytes: Arc<AtomicUsize>,
    pending_write_bytes: Arc<AtomicUsize>,
}

pub(crate) struct VtSession {
    client: VtWorkerClient,
    state: Arc<SessionState>,
}

struct SessionState {
    session_id: String,
    generation: String,
    scrollback: usize,
    status: AtomicU8,
    /// Advanced before a resize command can be observed by the actor.
    geometry_revision: AtomicU64,
    /// Geometry queued to the pipe; only a snapshot confirms it was applied.
    sent_revision: AtomicU64,
    /// End of the ordered byte stream sent to the worker, not an applied ACK.
    sent_offset: AtomicU64,
    sent_cols: AtomicU16,
    sent_rows: AtomicU16,
    dirty_bytes: AtomicUsize,
    snapshot: Mutex<Option<VtSnapshot>>,
    cached_snapshot_bytes: Arc<AtomicUsize>,
    pending_write: Mutex<Weak<Mutex<Option<WriteBatch>>>>,
}

struct WriteBatch {
    revision: u64,
    start_offset: u64,
    data: Vec<u8>,
    pending_bytes: Arc<AtomicUsize>,
}

impl WriteBatch {
    fn new(
        revision: u64,
        start_offset: u64,
        data: &[u8],
        pending_bytes: &Arc<AtomicUsize>,
    ) -> Option<Self> {
        if data.len() > MAX_WRITE_BATCH_BYTES || !reserve_write_bytes(pending_bytes, data.len()) {
            return None;
        }
        Some(Self {
            revision,
            start_offset,
            data: data.to_vec(),
            pending_bytes: pending_bytes.clone(),
        })
    }

    fn append(&mut self, revision: u64, start_offset: u64, data: &[u8]) -> bool {
        if revision != self.revision
            || self.start_offset.checked_add(self.data.len() as u64) != Some(start_offset)
            || data.len() > MAX_WRITE_BATCH_BYTES.saturating_sub(self.data.len())
            || !reserve_write_bytes(&self.pending_bytes, data.len())
        {
            return false;
        }
        self.data.extend_from_slice(data);
        true
    }
}

impl Drop for WriteBatch {
    fn drop(&mut self) {
        self.pending_bytes
            .fetch_sub(self.data.len(), Ordering::AcqRel);
    }
}

fn reserve_write_bytes(total: &AtomicUsize, bytes: usize) -> bool {
    total
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current
                .checked_add(bytes)
                .filter(|next| *next <= MAX_PENDING_WRITE_BYTES)
        })
        .is_ok()
}

enum WorkerCommand {
    Create {
        session: Arc<SessionState>,
        cols: u16,
        rows: u16,
    },
    Write {
        session: Arc<SessionState>,
        batch: Arc<Mutex<Option<WriteBatch>>>,
    },
    Resize {
        session: Arc<SessionState>,
        revision: u64,
        at_offset: u64,
        cols: u16,
        rows: u16,
    },
    Dispose {
        session: Arc<SessionState>,
    },
}

#[derive(Debug)]
enum RpcError {
    Io(io::Error),
    Timeout,
    Protocol(String),
    Session(String),
    Worker(String),
}

enum ActorOperation<T> {
    Complete(T),
    Shutdown,
    WorkerExited(String),
}

impl VtWorker {
    pub(crate) async fn start(config: VtWorkerConfig) -> io::Result<Self> {
        validate_config(&config)?;
        #[cfg(target_os = "macos")]
        let capture_stderr = aow_macos_log::requested();
        #[cfg(not(target_os = "macos"))]
        let capture_stderr = false;
        let mut child = Command::new(&config.program)
            .arg(&config.script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if capture_stderr {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .kill_on_drop(true)
            .spawn()?;
        #[cfg(target_os = "macos")]
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_worker_stderr(stderr));
        }
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("VT worker stdin pipe was not created"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("VT worker stdout pipe was not created"))?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let (shutdown, shutdown_changed) = watch::channel(false);
        let client = VtWorkerClient {
            sender,
            shutdown: Arc::new(shutdown),
            scrollback: config.scrollback,
            sessions: Arc::new(Mutex::new(Vec::new())),
            cached_snapshot_bytes: Arc::new(AtomicUsize::new(0)),
            pending_write_bytes: Arc::new(AtomicUsize::new(0)),
        };
        let task = tokio::spawn(run_worker_actor(
            child,
            RpcClient::new(
                stdin,
                stdout,
                config.request_timeout,
                client.sessions.clone(),
            ),
            receiver,
            shutdown_changed,
            config.snapshot_interval,
        ));
        Ok(Self {
            client,
            task: Some(task),
        })
    }

    pub(crate) fn client(&self) -> VtWorkerClient {
        self.client.clone()
    }

    pub(crate) fn shutdown_now(&self) {
        self.client.shutdown_now();
    }

    pub(crate) async fn shutdown(mut self) {
        self.shutdown_now();
        let Some(task) = self.task.take() else {
            return;
        };
        // Every actor operation observes the shutdown watch channel, and the
        // final child wait is independently bounded. Awaiting the actor keeps
        // the kill + reap guarantee instead of aborting while it owns Child.
        let _ = task.await;
    }
}

#[cfg(target_os = "macos")]
async fn forward_worker_stderr(stderr: tokio::process::ChildStderr) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        // Bound memory even if the worker emits an unterminated line. The
        // worker owns this pipe; its exit ends the task without an OS thread.
        match (&mut reader).take(4096).read_until(b'\n', &mut line).await {
            Ok(0) => break,
            Ok(_) => {
                tracing::warn!(message = %String::from_utf8_lossy(&line).trim_end(), "VT worker stderr")
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read VT worker stderr");
                break;
            }
        }
    }
}

impl Drop for VtWorker {
    fn drop(&mut self) {
        self.shutdown_now();
        // Dropping a JoinHandle detaches rather than cancels the task. The
        // actor therefore still invalidates sessions and performs its bounded
        // child kill/wait even if the caller future itself was cancelled.
        self.task.take();
    }
}

impl VtWorkerClient {
    fn shutdown_now(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.retain(|session| {
                if let Some(session) = session.upgrade() {
                    session.fail();
                    true
                } else {
                    false
                }
            });
        }
        self.shutdown.send_replace(true);
    }

    pub(crate) fn create_session(&self, generation: String, cols: u16, rows: u16) -> VtSession {
        let cols = normalize_vt_cols(cols);
        let state = Arc::new(SessionState {
            session_id: Uuid::new_v4().to_string(),
            generation,
            scrollback: self.scrollback,
            status: AtomicU8::new(SESSION_ACTIVE),
            geometry_revision: AtomicU64::new(0),
            sent_revision: AtomicU64::new(0),
            sent_offset: AtomicU64::new(0),
            sent_cols: AtomicU16::new(cols),
            sent_rows: AtomicU16::new(rows),
            dirty_bytes: AtomicUsize::new(0),
            snapshot: Mutex::new(None),
            cached_snapshot_bytes: self.cached_snapshot_bytes.clone(),
            pending_write: Mutex::new(Weak::new()),
        });
        let session = VtSession {
            client: self.clone(),
            state: state.clone(),
        };
        let registered = self.sessions.lock().is_ok_and(|mut sessions| {
            sessions.retain(|session| session.strong_count() != 0);
            sessions.push(Arc::downgrade(&state));
            true
        });
        if !registered {
            state.fail();
            return session;
        }
        session.enqueue(WorkerCommand::Create {
            session: state,
            cols,
            rows,
        });
        session
    }
}

impl VtSession {
    /// Queue output without waiting for Node or waiting for queue capacity.
    pub(crate) fn write(&self, start_offset: u64, data: &[u8]) {
        if data.is_empty() || !self.state.is_active() {
            return;
        }
        let Ok(mut pending) = self.state.pending_write.lock() else {
            self.state.fail();
            return;
        };
        let revision = self.state.geometry_revision.load(Ordering::Acquire);
        let mut offset = start_offset;
        for data in data.chunks(MAX_WRITE_BATCH_BYTES) {
            if !self.state.is_active() {
                return;
            }
            let Some(next_offset) = offset.checked_add(data.len() as u64) else {
                self.state.fail();
                return;
            };
            // The consumer takes the Option before writing to the pipe. Only a
            // batch it has not taken yet may grow; no timer delays idle input.
            // Other sessions may enqueue between these writes, but each
            // session's offsets and geometry revisions remain strictly ordered.
            let appended = pending.upgrade().is_some_and(|batch| {
                batch.lock().is_ok_and(|mut batch| {
                    batch
                        .as_mut()
                        .is_some_and(|batch| batch.append(revision, offset, data))
                })
            });
            if !appended {
                let Some(batch) =
                    WriteBatch::new(revision, offset, data, &self.client.pending_write_bytes)
                else {
                    self.state.fail();
                    tracing::warn!(
                        session_id = %self.state.session_id,
                        "VT worker pending output byte limit exceeded; falling back to raw terminal replay"
                    );
                    return;
                };
                let batch = Arc::new(Mutex::new(Some(batch)));
                *pending = Arc::downgrade(&batch);
                self.enqueue(WorkerCommand::Write {
                    session: self.state.clone(),
                    batch,
                });
            }
            offset = next_offset;
        }
    }

    /// Invalidate the cache before enqueueing resize. The revision barrier also
    /// rejects an old-size snapshot RPC that was already in flight.
    pub(crate) fn resize(&self, at_offset: u64, cols: u16, rows: u16) {
        if !self.state.is_active() {
            return;
        }
        let Some(revision) = self.state.invalidate_for_resize() else {
            return;
        };
        self.enqueue(WorkerCommand::Resize {
            session: self.state.clone(),
            revision,
            at_offset,
            cols: normalize_vt_cols(cols),
            rows,
        });
    }

    pub(crate) fn snapshot(&self) -> Option<VtSnapshot> {
        if !self.state.is_active() {
            return None;
        }
        let snapshot = self.state.snapshot.lock().ok()?.clone();
        self.state.is_active().then_some(snapshot).flatten()
    }

    pub(crate) fn dispose(&self) {
        if self
            .state
            .status
            .compare_exchange(
                SESSION_ACTIVE,
                SESSION_DISPOSING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        self.state.clear_snapshot();
        if let Err(error) = self.client.sender.try_send(WorkerCommand::Dispose {
            session: self.state.clone(),
        }) {
            self.state.fail();
            tracing::debug!(
                session_id = %self.state.session_id,
                %error,
                "VT worker dispose queue unavailable"
            );
        }
    }

    fn enqueue(&self, command: WorkerCommand) {
        if let Err(error) = self.client.sender.try_send(command) {
            self.state.fail();
            tracing::warn!(
                session_id = %self.state.session_id,
                %error,
                "VT worker queue unavailable; falling back to raw terminal replay"
            );
        }
    }
}

impl SessionState {
    fn is_active(&self) -> bool {
        self.status.load(Ordering::Acquire) == SESSION_ACTIVE
    }

    fn can_process(&self) -> bool {
        matches!(
            self.status.load(Ordering::Acquire),
            SESSION_ACTIVE | SESSION_DISPOSING
        )
    }

    fn fail(&self) {
        self.status.store(SESSION_FAILED, Ordering::Release);
        self.clear_snapshot();
    }

    fn invalidate_for_resize(&self) -> Option<u64> {
        let revision = match self.geometry_revision.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |revision| revision.checked_add(1),
        ) {
            Ok(previous) => previous + 1,
            Err(_) => {
                self.fail();
                return None;
            }
        };
        // Increment first: an old snapshot either commits before this clear
        // and is removed, or sees the new revision and cannot commit.
        self.clear_snapshot();
        Some(revision)
    }

    fn clear_snapshot(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock()
            && let Some(snapshot) = snapshot.take()
        {
            let previous = self
                .cached_snapshot_bytes
                .fetch_sub(snapshot.cached_bytes(), Ordering::AcqRel);
            debug_assert!(previous >= snapshot.cached_bytes());
        }
    }

    fn store_snapshot(&self, revision: u64, snapshot: VtSnapshot) -> bool {
        if !self.is_active()
            || self.geometry_revision.load(Ordering::Acquire) != revision
            || self.sent_revision.load(Ordering::Acquire) != revision
        {
            return false;
        }
        if let Ok(mut current) = self.snapshot.lock()
            && self.is_active()
            && self.geometry_revision.load(Ordering::Acquire) == revision
            && self.sent_revision.load(Ordering::Acquire) == revision
            && current
                .as_ref()
                .is_none_or(|existing| snapshot.applied_offset >= existing.applied_offset)
        {
            let old_bytes = current
                .as_ref()
                .map_or(0, |snapshot| snapshot.cached_bytes());
            let new_bytes = snapshot.cached_bytes();
            if new_bytes > old_bytes
                && !reserve_snapshot_bytes(&self.cached_snapshot_bytes, new_bytes - old_bytes)
            {
                return false;
            }
            *current = Some(snapshot);
            if old_bytes > new_bytes {
                let released = old_bytes - new_bytes;
                let previous = self
                    .cached_snapshot_bytes
                    .fetch_sub(released, Ordering::AcqRel);
                debug_assert!(previous >= released);
            }
            return true;
        }
        false
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        self.clear_snapshot();
    }
}

fn reserve_snapshot_bytes(total: &AtomicUsize, additional: usize) -> bool {
    additional == 0
        || total
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(additional)
                    .filter(|next| *next <= MAX_TOTAL_CACHED_SNAPSHOT_BYTES)
            })
            .is_ok()
}

async fn run_worker_actor(
    mut child: Child,
    mut rpc: RpcClient,
    mut receiver: mpsc::Receiver<WorkerCommand>,
    mut shutdown: watch::Receiver<bool>,
    snapshot_interval: Duration,
) {
    let mut sessions = Vec::<Weak<SessionState>>::new();
    let mut live_session_ids = std::collections::HashSet::<String>::new();
    let mut snapshot_cursor = 0;
    let mut snapshot_deadline = Instant::now() + snapshot_interval;
    let snapshot_tick = sleep_until(snapshot_deadline);
    tokio::pin!(snapshot_tick);
    let mut next_urgent_snapshot = Instant::now();
    let failure = loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    break None;
                }
            }
            status = child.wait() => {
                break Some(match status {
                    Ok(status) => format!("VT worker exited with {status}"),
                    Err(error) => format!("failed waiting for VT worker: {error}"),
                });
            }
            response = rpc.responses.recv() => {
                // Mutation errors are handled immediately by the reader. No
                // successful reply is expected unless an RPC is awaiting it.
                break Some(match response {
                    Some(Err(error)) => format_rpc_error(&error),
                    Some(Ok(_)) => "unsolicited VT worker response".to_owned(),
                    None => "VT worker response stream closed".to_owned(),
                });
            }
            // Keep the time threshold authoritative even when writes keep the
            // command queue continuously ready. The byte threshold remains an
            // earlier fast path for bursts larger than 256 KiB.
            _ = &mut snapshot_tick => {
                let operation = async {
                    let more_dirty = snapshot_dirty_sessions(
                        &mut rpc,
                        &mut sessions,
                        &live_session_ids,
                        &mut snapshot_cursor,
                    ).await?;
                    cleanup_failed_sessions(&mut rpc, &mut sessions, &mut live_session_ids).await?;
                    Ok::<_, RpcError>(more_dirty)
                };
                let result = await_actor_operation(&mut child, &mut shutdown, operation).await;
                // Do not immediately run another overdue pass after slow
                // serialization. Commands get a fresh interval in which to
                // drain before periodic work becomes eligible again.
                let completed_at = Instant::now();
                next_urgent_snapshot = completed_at + MIN_SNAPSHOT_PASS_SPACING;
                snapshot_deadline = if matches!(
                    &result,
                    ActorOperation::Complete(Ok(true))
                ) {
                    next_urgent_snapshot
                } else {
                    completed_at + snapshot_interval
                };
                snapshot_tick.as_mut().reset(snapshot_deadline);
                match result {
                    ActorOperation::Complete(Ok(_)) => {}
                    ActorOperation::Complete(Err(error)) => {
                        break Some(format_rpc_error(&error));
                    }
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                }
            }
            command = receiver.recv() => {
                let Some(command) = command else { break None };
                let state = command.session().clone();
                let kind = command.kind();
                if matches!(kind, WorkerCommandKind::Create) {
                    // Track before the compound create+initial-snapshot
                    // operation. If create succeeds but snapshot fails, the
                    // cleanup pass must still dispose the Node session.
                    sessions.push(Arc::downgrade(&state));
                    live_session_ids.insert(state.session_id.clone());
                }
                // An unresponsive Node RPC must not make daemon shutdown wait
                // for the normal request timeout. Cancelling the in-flight RPC
                // is safe here because the entire worker is about to be killed
                // and every session will fall back to raw replay.
                let result = match await_actor_operation(
                    &mut child,
                    &mut shutdown,
                    process_command(command, &mut rpc),
                ).await {
                    ActorOperation::Complete(result) => result,
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                };
                if let Ok(snapshot_urgent) = result {
                    if snapshot_urgent {
                        // Wake the shared bounded scheduler without allowing
                        // repeated large writes/resizes to serialize once per
                        // command. Repeated wakeups retain the earliest
                        // already-scheduled deadline.
                        let urgent_deadline = Instant::now().max(next_urgent_snapshot);
                        if urgent_deadline < snapshot_deadline {
                            snapshot_deadline = urgent_deadline;
                            snapshot_tick.as_mut().reset(snapshot_deadline);
                        }
                    }
                    match kind {
                        WorkerCommandKind::Create => {}
                        WorkerCommandKind::Dispose => {
                            live_session_ids.remove(&state.session_id);
                        }
                        WorkerCommandKind::Write | WorkerCommandKind::Resize => {}
                    }
                }
                if let Err(error) = result
                    && let Some(fatal) = handle_command_error(&state, &error)
                {
                    break Some(fatal);
                }
                match await_actor_operation(
                    &mut child,
                    &mut shutdown,
                    cleanup_failed_sessions(&mut rpc, &mut sessions, &mut live_session_ids),
                ).await {
                    ActorOperation::Complete(Ok(())) => {}
                    ActorOperation::Complete(Err(error)) => {
                        break Some(format_rpc_error(&error));
                    }
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                }
            }
        }
    };

    receiver.close();
    while let Ok(command) = receiver.try_recv() {
        command.session().fail();
    }
    for state in sessions.iter().filter_map(Weak::upgrade) {
        state.fail();
    }
    if let Some(failure) = failure {
        tracing::warn!(%failure, "VT worker stopped; terminal runtimes will use raw replay");
    }
    let _ = child.start_kill();
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
}

async fn await_actor_operation<T, F>(
    child: &mut Child,
    shutdown: &mut watch::Receiver<bool>,
    operation: F,
) -> ActorOperation<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(operation);
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    return ActorOperation::Shutdown;
                }
            }
            status = child.wait() => {
                return ActorOperation::WorkerExited(match status {
                    Ok(status) => format!("VT worker exited with {status}"),
                    Err(error) => format!("failed waiting for VT worker: {error}"),
                });
            }
            result = &mut operation => return ActorOperation::Complete(result),
        }
    }
}

async fn cleanup_failed_sessions(
    rpc: &mut RpcClient,
    sessions: &mut Vec<Weak<SessionState>>,
    live_session_ids: &mut std::collections::HashSet<String>,
) -> Result<(), RpcError> {
    let failed = sessions
        .iter()
        .filter_map(Weak::upgrade)
        .filter(|session| {
            session.status.load(Ordering::Acquire) == SESSION_FAILED
                && live_session_ids.contains(&session.session_id)
        })
        .collect::<Vec<_>>();
    for session in failed {
        match rpc
            .call(
                "dispose",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(RpcError::Worker(message) | RpcError::Session(message)) => {
                tracing::debug!(
                    session_id = %session.session_id,
                    %message,
                    "failed VT session was already unavailable during cleanup"
                );
            }
            Err(error) => return Err(error),
        }
        live_session_ids.remove(&session.session_id);
    }
    sessions.retain(|session| {
        session
            .upgrade()
            .is_some_and(|state| live_session_ids.contains(&state.session_id))
    });
    Ok(())
}

fn handle_command_error(state: &SessionState, error: &RpcError) -> Option<String> {
    state.fail();
    match error {
        RpcError::Worker(message) | RpcError::Session(message) => {
            tracing::warn!(
                session_id = %state.session_id,
                %message,
                "VT worker rejected a session operation; using raw replay"
            );
            None
        }
        RpcError::Io(_) | RpcError::Timeout | RpcError::Protocol(_) => {
            Some(format_rpc_error(error))
        }
    }
}

async fn snapshot_dirty_sessions(
    rpc: &mut RpcClient,
    sessions: &mut Vec<Weak<SessionState>>,
    live_session_ids: &std::collections::HashSet<String>,
    cursor: &mut usize,
) -> Result<bool, RpcError> {
    sessions.retain(|session| session.strong_count() != 0);
    if sessions.is_empty() {
        *cursor = 0;
        return Ok(false);
    }
    *cursor %= sessions.len();
    let scan_limit = sessions.len();
    let started = Instant::now();
    let mut scanned = 0;
    let mut snapshotted = 0;

    while scanned < scan_limit && snapshotted < MAX_SNAPSHOTS_PER_TICK {
        if snapshotted != 0 && started.elapsed() >= SNAPSHOT_TICK_BUDGET {
            break;
        }
        let index = *cursor;
        *cursor = (*cursor + 1) % sessions.len();
        scanned += 1;
        let Some(session) = sessions[index].upgrade() else {
            continue;
        };
        if !live_session_ids.contains(&session.session_id)
            || !session.is_active()
            || session.dirty_bytes.load(Ordering::Acquire) == 0
        {
            continue;
        }
        snapshotted += 1;
        let revision = session.sent_revision.load(Ordering::Acquire);
        if let Err(error) = snapshot_session(rpc, &session, revision).await {
            session.fail();
            match error {
                RpcError::Worker(message) | RpcError::Session(message) => {
                    tracing::warn!(
                        session_id = %session.session_id,
                        %message,
                        "VT worker rejected a snapshot operation; using raw replay"
                    );
                }
                other => return Err(other),
            }
        }
    }
    Ok(sessions.iter().filter_map(Weak::upgrade).any(|session| {
        live_session_ids.contains(&session.session_id)
            && session.is_active()
            && session.dirty_bytes.load(Ordering::Acquire) != 0
    }))
}

impl WorkerCommand {
    fn session(&self) -> &Arc<SessionState> {
        match self {
            Self::Create { session, .. }
            | Self::Write { session, .. }
            | Self::Resize { session, .. }
            | Self::Dispose { session } => session,
        }
    }

    fn kind(&self) -> WorkerCommandKind {
        match self {
            Self::Create { .. } => WorkerCommandKind::Create,
            Self::Write { .. } => WorkerCommandKind::Write,
            Self::Resize { .. } => WorkerCommandKind::Resize,
            Self::Dispose { .. } => WorkerCommandKind::Dispose,
        }
    }
}

#[derive(Clone, Copy)]
enum WorkerCommandKind {
    Create,
    Write,
    Resize,
    Dispose,
}

/// Process one ordered mutation and report whether the bounded snapshot
/// scheduler should be woken immediately. No command serializes terminal
/// state inline; that keeps burst writes and resize storms on the same global
/// work budget as periodic snapshots.
async fn process_command(command: WorkerCommand, rpc: &mut RpcClient) -> Result<bool, RpcError> {
    match command {
        WorkerCommand::Create {
            session,
            cols,
            rows,
        } => {
            if !session.can_process() {
                return Ok(false);
            }
            rpc.call(
                "create",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "initial_offset": 0,
                    "cols": cols,
                    "rows": rows,
                    "scrollback": session.scrollback,
                }),
            )
            .await?;
            session.dirty_bytes.store(1, Ordering::Release);
            Ok(true)
        }
        WorkerCommand::Write { session, batch } => {
            let batch = batch
                .lock()
                .map_err(|_| RpcError::Session("write batch lock poisoned".to_owned()))?
                .take()
                .ok_or_else(|| RpcError::Session("write batch already consumed".to_owned()))?;
            if !session.can_process() {
                return Ok(false);
            }
            validate_sent_revision(&session, batch.revision)?;
            validate_js_offset(batch.start_offset, "start_offset")?;
            let byte_length = batch.data.len();
            let expected_offset = session.sent_offset.load(Ordering::Acquire);
            if batch.start_offset != expected_offset {
                return Err(RpcError::Session(
                    "non-contiguous VT write offset".to_owned(),
                ));
            }
            let end_offset = batch
                .start_offset
                .checked_add(byte_length as u64)
                .ok_or_else(|| RpcError::Session("VT write offset overflow".to_owned()))?;
            validate_js_offset(end_offset, "end_offset")?;
            rpc.send(
                "write",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "start_offset": batch.start_offset,
                }),
                &batch.data,
            )
            .await?;
            session.sent_offset.store(end_offset, Ordering::Release);
            let dirty = session
                .dirty_bytes
                .fetch_add(byte_length, Ordering::AcqRel)
                .saturating_add(byte_length);
            Ok(dirty >= SNAPSHOT_BYTE_INTERVAL)
        }
        WorkerCommand::Resize {
            session,
            revision,
            at_offset,
            cols,
            rows,
        } => {
            if !session.can_process() {
                return Ok(false);
            }
            let previous_revision = revision
                .checked_sub(1)
                .ok_or_else(|| RpcError::Session("resize revision underflow".to_owned()))?;
            validate_sent_revision(&session, previous_revision)?;
            validate_js_offset(at_offset, "at_offset")?;
            if at_offset != session.sent_offset.load(Ordering::Acquire) {
                return Err(RpcError::Session(
                    "resize does not match sent offset".to_owned(),
                ));
            }
            rpc.send(
                "resize",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "at_offset": at_offset,
                    "cols": cols,
                    "rows": rows,
                }),
                &[],
            )
            .await?;
            session.sent_cols.store(cols, Ordering::Release);
            session.sent_rows.store(rows, Ordering::Release);
            session.sent_revision.store(revision, Ordering::Release);
            // Resize invalidated the old cache synchronously. Mark the new
            // geometry dirty and wake the same bounded scheduler used for
            // every other snapshot.
            session.dirty_bytes.store(1, Ordering::Release);
            Ok(true)
        }
        WorkerCommand::Dispose { session } => {
            if !session.can_process() {
                return Ok(false);
            }
            rpc.call(
                "dispose",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                }),
            )
            .await?;
            session.status.store(SESSION_DISPOSED, Ordering::Release);
            session.clear_snapshot();
            Ok(false)
        }
    }
}

fn validate_sent_revision(session: &SessionState, expected: u64) -> Result<(), RpcError> {
    let actual = session.sent_revision.load(Ordering::Acquire);
    if actual != expected {
        return Err(RpcError::Session(format!(
            "VT session worker revision {actual}, expected {expected}"
        )));
    }
    Ok(())
}

async fn snapshot_session(
    rpc: &mut RpcClient,
    session: &Arc<SessionState>,
    revision: u64,
) -> Result<(), RpcError> {
    if !session.can_process() {
        return Ok(());
    }
    validate_sent_revision(session, revision)?;
    let expected_offset = session.sent_offset.load(Ordering::Acquire);
    let response = rpc
        .call(
            "snapshot",
            json!({
                "session_id": session.session_id,
                "generation": session.generation,
                "scrollback": session.scrollback,
            }),
        )
        .await?;
    let mut value = response.metadata;
    validate_exact_offset(&value, expected_offset)?;
    let applied_offset = expected_offset;
    let cols = result_dimension(&value, "cols")?;
    let rows = result_dimension(&value, "rows")?;
    if cols != session.sent_cols.load(Ordering::Acquire)
        || rows != session.sent_rows.load(Ordering::Acquire)
    {
        return Err(RpcError::Session(
            "snapshot geometry does not match ordered resize".to_owned(),
        ));
    }
    if response.data.len() > MAX_CACHED_SNAPSHOT_BYTES {
        return Err(RpcError::Session(format!(
            "snapshot exceeds the {MAX_CACHED_SNAPSHOT_BYTES} byte cache limit"
        )));
    }
    if result_u64(&value, "byte_length")? != response.data.len() as u64 {
        return Err(RpcError::Protocol(
            "snapshot payload length mismatch".to_owned(),
        ));
    }
    let ansi = String::from_utf8(response.data)
        .map_err(|_| RpcError::Protocol("snapshot data is not valid UTF-8".to_owned()))?;
    let lines: Vec<String> = serde_json::from_value(
        value
            .get_mut("lines")
            .map(Value::take)
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|_| RpcError::Protocol("invalid viewport lines".to_owned()))?;
    if lines.len() > usize::from(rows)
        || lines.iter().map(String::len).sum::<usize>() > MAX_CACHED_SNAPSHOT_BYTES
    {
        return Err(RpcError::Session(
            "viewport exceeds snapshot limits".to_owned(),
        ));
    }
    session.dirty_bytes.store(0, Ordering::Release);
    session.store_snapshot(
        revision,
        VtSnapshot {
            generation: session.generation.clone(),
            applied_offset,
            cols,
            rows,
            ansi,
            lines,
        },
    );
    Ok(())
}

// Keep aligned with vt-worker/src/protocol.mjs. Xterm needs at least two
// columns for wide characters. Normalize valid one-column requests before
// queueing them; invalid zero dimensions must still be rejected by the worker.
fn normalize_vt_cols(cols: u16) -> u16 {
    if cols == 1 { 2 } else { cols }
}

fn validate_config(config: &VtWorkerConfig) -> io::Result<()> {
    if config.queue_capacity == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VT worker queue capacity must be positive",
        ));
    }
    if config.request_timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VT worker request timeout must be positive",
        ));
    }
    if config.snapshot_interval.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VT worker snapshot interval must be positive",
        ));
    }
    if config.scrollback > MAX_SCROLLBACK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("VT worker scrollback cannot exceed {MAX_SCROLLBACK}"),
        ));
    }
    Ok(())
}

fn validate_exact_offset(value: &Value, expected: u64) -> Result<(), RpcError> {
    validate_js_offset(expected, "expected applied_offset")?;
    let actual = result_u64(value, "applied_offset")?;
    if actual != expected {
        return Err(RpcError::Session(format!(
            "worker applied offset {actual}, expected {expected}"
        )));
    }
    Ok(())
}

fn validate_js_offset(value: u64, name: &str) -> Result<(), RpcError> {
    if value > MAX_JS_SAFE_INTEGER {
        return Err(RpcError::Session(format!(
            "{name} exceeds the JavaScript safe integer range"
        )));
    }
    Ok(())
}

fn result_u64(value: &Value, name: &str) -> Result<u64, RpcError> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::Protocol(format!("result omitted integer {name}")))
}

fn result_dimension(value: &Value, name: &str) -> Result<u16, RpcError> {
    let value = result_u64(value, name)?;
    if !(1..=1_000).contains(&value) {
        return Err(RpcError::Protocol(format!(
            "result {name} must be between 1 and 1000"
        )));
    }
    Ok(value as u16)
}

fn format_rpc_error(error: &RpcError) -> String {
    match error {
        RpcError::Io(error) => error.to_string(),
        RpcError::Timeout => "VT worker RPC timed out".to_owned(),
        RpcError::Protocol(message) | RpcError::Session(message) | RpcError::Worker(message) => {
            message.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let session = worker.client().create_session(
                format!("one-column-{initial_cols}"),
                initial_cols,
                1,
            );
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
        let framing =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/src/framing.mjs");
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
        let framing =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/src/framing.mjs");
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
}
