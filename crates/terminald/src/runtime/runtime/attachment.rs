use super::*;

pub(in crate::runtime) struct ControllerState {
    pub(in crate::runtime) generation: u64,
    pub(in crate::runtime) owner: Option<ControllerOwner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) struct ControllerOwner {
    pub(in crate::runtime) attachment_id: u128,
    pub(in crate::runtime) generation: u64,
}

pub(in crate::runtime) struct RuntimeConnection {
    pub(in crate::runtime) runtime: Arc<Runtime>,
    pub(in crate::runtime) attachment_id: u128,
    pub(in crate::runtime) resume_after: Option<u64>,
    pub(in crate::runtime) vt_snapshot: bool,
    pub(in crate::runtime) deleted_changed: watch::Receiver<bool>,
    pub(in crate::runtime) controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

pub(in crate::runtime) struct ClaimedAttachment {
    pub(in crate::runtime) stream_epoch: String,
    pub(in crate::runtime) stream_offset: u64,
    pub(in crate::runtime) reset: bool,
    pub(in crate::runtime) replay: Vec<u8>,
    pub(in crate::runtime) replay_bytes: u64,
    pub(in crate::runtime) next_offset: u64,
    pub(in crate::runtime) owner: ControllerOwner,
    pub(in crate::runtime) controller_changed: watch::Receiver<Option<ControllerOwner>>,
    pub(in crate::runtime) restore: Option<String>,
    pub(in crate::runtime) restore_cols: Option<u16>,
    pub(in crate::runtime) restore_rows: Option<u16>,
}

/// A snapshot and output subscription for an attachment that may read the
/// terminal but never owns its input or PTY dimensions.
pub(in crate::runtime) struct ObservedAttachment {
    pub(in crate::runtime) stream_epoch: String,
    pub(in crate::runtime) stream_offset: u64,
    pub(in crate::runtime) reset: bool,
    pub(in crate::runtime) replay: Vec<u8>,
    pub(in crate::runtime) replay_bytes: u64,
    pub(in crate::runtime) next_offset: u64,
    pub(in crate::runtime) restore: Option<String>,
    pub(in crate::runtime) restore_cols: Option<u16>,
    pub(in crate::runtime) restore_rows: Option<u16>,
    pub(in crate::runtime) controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

pub(in crate::runtime) enum ClaimOutcome {
    Claimed(ClaimedAttachment),
    Waiting,
    Unchanged,
}

impl Runtime {
    pub(in crate::runtime) fn description(&self) -> Result<TerminalRuntime, TerminaldError> {
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

    pub(in crate::runtime) fn creation_spec(&self) -> Result<TerminalRuntimeSpec, TerminaldError> {
        Ok(self.creation_spec.clone())
    }

    pub(in crate::runtime) fn is_deleted(&self) -> bool {
        self.deleted.load(Ordering::Acquire)
    }

    pub(in crate::runtime) fn connection(
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
            attachment_id: u128::from(aow_id::new_snowflake().as_u64()),
            resume_after: after,
            vt_snapshot,
            deleted_changed,
            controller_changed: self.controller_changed.subscribe(),
        })
    }

    pub(in crate::runtime) fn claim(
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
    pub(in crate::runtime) fn observe(
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

    pub(in crate::runtime) fn is_owner(
        &self,
        owner: ControllerOwner,
    ) -> Result<bool, TerminaldError> {
        let controller = self
            .controller
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        Ok(controller.owner == Some(owner))
    }

    pub(in crate::runtime) fn release(&self, owner: ControllerOwner) {
        let Ok(mut controller) = self.controller.lock() else {
            return;
        };
        if controller.owner == Some(owner) {
            controller.owner = None;
            self.controller_changed.send_replace(None);
        }
    }
}
