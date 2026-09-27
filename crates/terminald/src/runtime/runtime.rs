//! Per-runtime lifecycle, control ownership, and I/O coordination.

use super::*;

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

pub(super) struct ControllerState {
    pub(super) generation: u64,
    pub(super) owner: Option<ControllerOwner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ControllerOwner {
    pub(super) attachment_id: u128,
    pub(super) generation: u64,
}

pub(super) struct RuntimeConnection {
    pub(super) runtime: Arc<Runtime>,
    pub(super) attachment_id: u128,
    pub(super) resume_after: Option<u64>,
    pub(super) vt_snapshot: bool,
    pub(super) deleted_changed: watch::Receiver<bool>,
    pub(super) controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

pub(super) struct ClaimedAttachment {
    pub(super) stream_epoch: String,
    pub(super) stream_offset: u64,
    pub(super) reset: bool,
    pub(super) replay: Vec<u8>,
    pub(super) replay_bytes: u64,
    pub(super) next_offset: u64,
    pub(super) owner: ControllerOwner,
    pub(super) controller_changed: watch::Receiver<Option<ControllerOwner>>,
    pub(super) restore: Option<String>,
    pub(super) restore_cols: Option<u16>,
    pub(super) restore_rows: Option<u16>,
}

/// A snapshot and output subscription for an attachment that may read the
/// terminal but never owns its input or PTY dimensions.
pub(super) struct ObservedAttachment {
    pub(super) stream_epoch: String,
    pub(super) stream_offset: u64,
    pub(super) reset: bool,
    pub(super) replay: Vec<u8>,
    pub(super) replay_bytes: u64,
    pub(super) next_offset: u64,
    pub(super) restore: Option<String>,
    pub(super) restore_cols: Option<u16>,
    pub(super) restore_rows: Option<u16>,
    pub(super) controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

pub(super) enum ClaimOutcome {
    Claimed(ClaimedAttachment),
    Waiting,
    Unchanged,
}

impl Runtime {
    pub(super) fn description(&self) -> Result<TerminalRuntime, TerminaldError> {
        let metadata = self.metadata.lock().map_err(|_| TerminaldError::Poisoned)?;
        Ok(TerminalRuntime {
            id: self.id.clone(),
            cwd: self.creation_spec.cwd.clone(),
            shell: self.creation_spec.shell.clone(),
            arguments: self.creation_spec.arguments.clone(),
            environment: self.creation_spec.environment.clone(),
            status: metadata.status,
            rows: metadata.rows,
            cols: metadata.cols,
            exit_code: metadata.exit_code,
        })
    }

    pub(super) fn creation_spec(&self) -> Result<TerminalRuntimeSpec, TerminaldError> {
        Ok(self.creation_spec.clone())
    }

    pub(super) fn is_deleted(&self) -> bool {
        self.deleted.load(Ordering::Acquire)
    }

    pub(super) fn connection(
        self: &Arc<Self>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<RuntimeConnection, TerminaldError> {
        let deleted_changed = self.deleted_changed.subscribe();
        if self.is_deleted() {
            return Err(TerminaldError::NotFound(self.id.clone()));
        }
        Ok(RuntimeConnection {
            runtime: self.clone(),
            attachment_id: Uuid::new_v4().as_u128(),
            resume_after: after,
            vt_snapshot,
            deleted_changed,
            controller_changed: self.controller_changed.subscribe(),
        })
    }

    pub(super) fn claim(
        self: &Arc<Self>,
        attachment_id: u128,
        after: Option<u64>,
        force: bool,
        vt_snapshot: bool,
    ) -> Result<ClaimOutcome, TerminaldError> {
        let mut controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if controller
            .owner
            .is_some_and(|owner| owner.attachment_id == attachment_id)
        {
            return Ok(ClaimOutcome::Unchanged);
        }
        if controller
            .owner
            .is_some_and(|owner| owner.attachment_id != attachment_id)
            && !force
        {
            return Ok(ClaimOutcome::Waiting);
        }
        if self.is_deleted() {
            return Err(TerminaldError::NotFound(self.id.clone()));
        }
        // Output producers hold this same lock while broadcasting, so the
        // snapshot has an exact byte boundary for later catch-up.
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        if self.is_deleted() {
            return Err(TerminaldError::NotFound(self.id.clone()));
        }

        let (vt_snapshot, snapshot_unavailable_reason) = if !vt_snapshot {
            (None, "capability_not_requested")
        } else if let Some(session) = &self.vt_session {
            (session.snapshot(), "snapshot_not_ready")
        } else {
            (None, "worker_unavailable")
        };
        let snapshot = output.snapshot_with_vt_diagnostics(
            after,
            vt_snapshot,
            &self.stream_epoch,
            snapshot_unavailable_reason,
        );
        let replay_bytes =
            u64::try_from(snapshot.replay.len()).expect("scrollback length fits in u64");
        tracing::debug!(
            target: "terminal_restore",
            runtime_id = %self.id,
            resume_requested = after.is_some(),
            reset = snapshot.reset,
            snapshot_selected = snapshot.restore_diagnostics.snapshot_selected,
            reason = snapshot.restore_diagnostics.reason,
            snapshot_bytes = snapshot.restore_diagnostics.snapshot_bytes,
            snapshot_offset = ?snapshot.restore_diagnostics.snapshot_offset,
            next_offset = snapshot.next_offset,
            replay_bytes,
            "terminal restore prepared"
        );

        controller.generation = controller
            .generation
            .checked_add(1)
            .ok_or_else(|| TerminaldError::Worker("controller generation overflow".to_owned()))?;
        let owner = ControllerOwner {
            attachment_id,
            generation: controller.generation,
        };
        controller.owner = Some(owner);
        self.controller_changed.send_replace(Some(owner));
        let controller_changed = self.controller_changed.subscribe();
        drop(output);
        drop(controller);
        Ok(ClaimOutcome::Claimed(ClaimedAttachment {
            stream_epoch: self.stream_epoch.clone(),
            stream_offset: snapshot.offset,
            reset: snapshot.reset,
            replay: snapshot.replay,
            replay_bytes,
            next_offset: snapshot.next_offset,
            owner,
            controller_changed,
            restore: snapshot.restore,
            restore_cols: snapshot.restore_cols,
            restore_rows: snapshot.restore_rows,
        }))
    }

    /// Prepare a read-only attachment without changing controller ownership.
    /// The output lock gives the initial replay and future broadcast receiver
    /// the same contiguous boundary as a controller attachment.
    pub(super) fn observe(
        &self,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<ObservedAttachment, TerminaldError> {
        if self.is_deleted() {
            return Err(TerminaldError::NotFound(self.id.clone()));
        }
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        if self.is_deleted() {
            return Err(TerminaldError::NotFound(self.id.clone()));
        }
        let (vt_snapshot, snapshot_unavailable_reason) = if !vt_snapshot {
            (None, "capability_not_requested")
        } else if let Some(session) = &self.vt_session {
            (session.snapshot(), "snapshot_not_ready")
        } else {
            (None, "worker_unavailable")
        };
        let snapshot = output.snapshot_with_vt_diagnostics(
            after,
            vt_snapshot,
            &self.stream_epoch,
            snapshot_unavailable_reason,
        );
        let replay_bytes =
            u64::try_from(snapshot.replay.len()).expect("scrollback length fits in u64");
        Ok(ObservedAttachment {
            stream_epoch: self.stream_epoch.clone(),
            stream_offset: snapshot.offset,
            reset: snapshot.reset,
            replay: snapshot.replay,
            replay_bytes,
            next_offset: snapshot.next_offset,
            restore: snapshot.restore,
            restore_cols: snapshot.restore_cols,
            restore_rows: snapshot.restore_rows,
            controller_changed: self.controller_changed.subscribe(),
        })
    }

    pub(super) fn append_output(&self, bytes: &[u8]) -> Result<(), TerminaldError> {
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        {
            let OutputState {
                state_parser,
                terminal,
                ..
            } = &mut *output;
            state_parser.advance(terminal, bytes);
        }
        let offset = output.append(bytes, SCROLLBACK_LIMIT)?;
        if let Some(vt_session) = &self.vt_session {
            vt_session.write(offset, bytes);
        }
        let _ = self.events.send(RuntimeEvent::Output {
            offset,
            bytes: Bytes::copy_from_slice(bytes),
        });
        Ok(())
    }

    pub(super) fn close_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            output.closed = true;
            let _ = self.events.send(RuntimeEvent::OutputClosed);
        }
    }

    pub(super) fn mark_reaped(
        &self,
        status: TerminalPaneStatus,
        exit_code: Option<u32>,
        kill_remaining_session: bool,
    ) {
        // Serialize the post-wait session cleanup with DELETE's PID-based kill
        // path. Until this guard is released, DELETE cannot observe `reaped`
        // false and act on a PID that the OS may already be able to reuse.
        let Ok(mut reaped) = self.reap_state.lock() else {
            return;
        };
        if kill_remaining_session {
            self.kill_remaining_session_members();
        }
        if !self.deleted.load(Ordering::Acquire) {
            if let Ok(mut metadata) = self.metadata.lock() {
                metadata.status = status;
                metadata.exit_code = exit_code;
            }
            let _ = self.events.send(RuntimeEvent::Status { status, exit_code });
        }
        *reaped = true;
        self.reaped.notify_all();
    }

    pub(super) fn is_owner(&self, owner: ControllerOwner) -> Result<bool, TerminaldError> {
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        Ok(controller.owner == Some(owner))
    }

    pub(super) fn release(&self, owner: ControllerOwner) {
        let Ok(mut controller) = self.controller.lock() else {
            return;
        };
        if controller.owner == Some(owner) {
            controller.owner = None;
            self.controller_changed.send_replace(None);
        }
    }

    pub(super) fn snapshot_and_subscribe(
        &self,
        after: u64,
    ) -> Result<(OutputSnapshot, broadcast::Receiver<RuntimeEvent>), TerminaldError> {
        // Output producers hold this same lock while broadcasting, so the
        // snapshot and replacement receiver form one atomic stream boundary.
        let mut output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        let events = self.events.subscribe();
        let snapshot = output.snapshot(Some(after));
        Ok((snapshot, events))
    }

    pub(super) fn write_input(
        &self,
        owner: ControllerOwner,
        bytes: &[u8],
    ) -> Result<bool, TerminaldError> {
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if controller.owner != Some(owner) {
            return Ok(false);
        }
        let mut writer = self.writer.lock().map_err(|_| TerminaldError::Poisoned)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(true)
    }

    pub(super) fn resize(
        &self,
        owner: ControllerOwner,
        rows: u16,
        cols: u16,
    ) -> Result<bool, TerminaldError> {
        validate_dimensions(rows, cols)?;
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if controller.owner != Some(owner) {
            return Ok(false);
        }
        {
            let metadata = self.metadata.lock().map_err(|_| TerminaldError::Poisoned)?;
            if metadata.status != TerminalPaneStatus::Running {
                return Err(TerminaldError::Conflict(self.id.clone()));
            }
        }
        // Output append uses this same lock, making the VT resize offset an
        // exact boundary relative to every queued write. Node RPC remains
        // asynchronous; no standard mutex is held while it runs.
        let output = self.output.lock().map_err(|_| TerminaldError::Poisoned)?;
        self.master
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminaldError::Pty(error.to_string()))?;
        if let Some(vt_session) = &self.vt_session {
            vt_session.resize(output.next_offset, cols, rows);
        }
        drop(output);
        let mut metadata = self.metadata.lock().map_err(|_| TerminaldError::Poisoned)?;
        metadata.rows = rows;
        metadata.cols = cols;
        Ok(true)
    }

    pub(super) fn notify_deleted(&self) {
        if !self.deleted.swap(true, Ordering::AcqRel) {
            if let Some(vt_session) = &self.vt_session {
                vt_session.dispose();
            }
            self.deleted_changed.send_replace(true);
            let _ = self.events.send(RuntimeEvent::Deleted);
        }
    }

    pub(super) fn force_delete_and_reap(&self) -> Result<(), TerminaldError> {
        self.notify_deleted();
        self.kill_best_effort();
        let reaped = self
            .reap_state
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if *reaped {
            return Ok(());
        }
        let (reaped, timeout) = self
            .reaped
            .wait_timeout_while(reaped, DELETE_REAP_TIMEOUT, |reaped| !*reaped)
            .map_err(|_| TerminaldError::Poisoned)?;
        if timeout.timed_out() && !*reaped {
            return Err(TerminaldError::Worker(format!(
                "timed out reaping runtime {}",
                self.id
            )));
        }
        Ok(())
    }

    pub(super) fn kill_best_effort(&self) {
        if self.reap_state.lock().is_ok_and(|reaped| *reaped) {
            return;
        }
        let foreground_process_group = self
            .master
            .lock()
            .ok()
            .and_then(|master| master.process_group_leader());
        unsafe {
            if let Some(process_group) = foreground_process_group
                && process_group > 1
            {
                libc::kill(-process_group, libc::SIGKILL);
            }
            if let Some(pid) = self.child_pid
                && let Ok(pid) = i32::try_from(pid)
                && pid > 1
            {
                #[cfg(target_os = "linux")]
                if let Some(session_id) = self.child_session_id {
                    for member in linux_session_members(session_id) {
                        libc::kill(member, libc::SIGKILL);
                    }
                }
                #[cfg(target_os = "macos")]
                if let Some(session_id) = self.child_session_id {
                    macos_kill_session_members(session_id, self.child_start_time.as_deref());
                }
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        }
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    pub(super) fn kill_remaining_session_members(&self) {
        let Some(session_id) = self.child_session_id else {
            return;
        };
        #[cfg(target_os = "linux")]
        unsafe {
            // The PTY child is the session leader. `child.wait()` has already
            // reaped it, so every remaining member is a descendant that must
            // not outlive the completed runtime.
            for member in linux_session_members(session_id) {
                libc::kill(member, libc::SIGKILL);
            }
        }
        #[cfg(target_os = "macos")]
        macos_kill_session_members(session_id, self.child_start_time.as_deref());
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        unsafe {
            // Portable best effort for Unix systems without /proc session
            // enumeration. This reaches jobs still in the leader's group.
            libc::kill(-session_id, libc::SIGKILL);
        }
    }
}
