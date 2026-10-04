//! Per-runtime lifecycle, control ownership, and I/O coordination.

mod attachment;
mod input;
mod lifecycle;
mod output;

use super::*;

pub(super) use attachment::{
    ClaimOutcome, ClaimedAttachment, ControllerOwner, ControllerState, ObservedAttachment,
    RuntimeConnection,
};

pub(super) struct Runtime {
    pub(super) id: String,
    pub(super) stream_epoch: String,
    pub(super) creation_spec: TerminalRuntimeSpec,
    pub(super) metadata: Mutex<RuntimeMetadata>,
    pub(super) master: Mutex<Box<dyn MasterPty + Send>>,
    pub(super) writer: Mutex<Box<dyn Write + Send>>,
    pub(super) killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pub(super) child_pid: Option<u32>,
    pub(super) child_session_id: Option<i32>,
    #[cfg(target_os = "macos")]
    pub(super) child_start_time: Option<String>,
    pub(super) deleted: AtomicBool,
    pub(super) output: Mutex<OutputState>,
    pub(super) vt_session: Option<VtSession>,
    pub(super) events: broadcast::Sender<RuntimeEvent>,
    pub(super) controller: Mutex<ControllerState>,
    pub(super) controller_changed: watch::Sender<Option<ControllerOwner>>,
    pub(super) delivery_gate: AsyncMutex<()>,
    pub(super) deleted_changed: watch::Sender<bool>,
    pub(super) reap_state: Mutex<bool>,
    pub(super) reaped: Condvar,
}

#[derive(Clone)]
pub(super) struct RuntimeMetadata {
    pub(super) rows: u16,
    pub(super) cols: u16,
    pub(super) status: TerminalPaneStatus,
    pub(super) exit_code: Option<u32>,
}
