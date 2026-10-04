use std::{io, path::PathBuf, time::Duration};

pub(super) const DEFAULT_QUEUE_CAPACITY: usize = 2_048;
pub(super) const MAX_WRITE_BATCH_BYTES: usize = 64 * 1024;
// Previously each queue entry held at most one 8 KiB PTY read. Keep that
// aggregate byte bound when multiple reads can share a queued command.
// A sent batch releases its reservation once the pipe accepts it. The worker
// reads/executes one frame at a time, so pipe backpressure bounds the rest.
pub(super) const MAX_PENDING_WRITE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const DEFAULT_SCROLLBACK: usize = 100;
pub(super) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const DEFAULT_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(200);
// Keep this aligned with vt-worker/src/vt-service.mjs.
pub(super) const MAX_SCROLLBACK: usize = 100;
// Keep this aligned with vt-worker/src/protocol.mjs.
pub(super) const MAX_CACHED_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_TOTAL_CACHED_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
pub(super) const SNAPSHOT_BYTE_INTERVAL: usize = 256 * 1024;
// Snapshot work shares one ordered actor with write and resize RPCs. Bound
// each timer pass so a large set of dirty sessions cannot monopolize the
// worker or fill the non-blocking command queue. The cursor below makes the
// bounded pass fair across ticks.
pub(super) const MAX_SNAPSHOTS_PER_TICK: usize = 4;
pub(super) const SNAPSHOT_TICK_BUDGET: Duration = Duration::from_millis(50);
// A byte-threshold or resize wakeup may bring the normal 200ms deadline
// forward, but it cannot create an unbounded serialize-per-command loop.
pub(super) const MIN_SNAPSHOT_PASS_SPACING: Duration = Duration::from_millis(50);
pub(super) const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) const SESSION_ACTIVE: u8 = 0;
pub(super) const SESSION_DISPOSING: u8 = 1;
pub(super) const SESSION_DISPOSED: u8 = 2;
pub(super) const SESSION_FAILED: u8 = 3;

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

pub(super) fn normalize_vt_cols(cols: u16) -> u16 {
    if cols == 1 { 2 } else { cols }
}
pub(super) fn validate_config(config: &VtWorkerConfig) -> io::Result<()> {
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
