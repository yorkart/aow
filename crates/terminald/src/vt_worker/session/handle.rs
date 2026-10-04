use std::sync::{Arc, Mutex, atomic::Ordering};

use super::{
    super::{
        config::{MAX_WRITE_BATCH_BYTES, SESSION_ACTIVE, SESSION_DISPOSING, normalize_vt_cols},
        queue::{WorkerCommand, WriteBatch},
    },
    client::VtWorkerClient,
    state::{SessionState, VtSnapshot},
};

pub(crate) struct VtSession {
    pub(in crate::vt_worker) client: VtWorkerClient,
    pub(in crate::vt_worker) state: Arc<SessionState>,
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

    pub(super) fn enqueue(&self, command: WorkerCommand) {
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
