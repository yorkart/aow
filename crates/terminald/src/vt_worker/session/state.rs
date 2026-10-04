use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU8, AtomicU16, AtomicU64, AtomicUsize, Ordering},
};

use super::super::{
    config::{SESSION_ACTIVE, SESSION_DISPOSING, SESSION_FAILED},
    queue::WriteBatch,
    snapshot::reserve_snapshot_bytes,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VtSnapshot {
    pub(crate) generation: String,
    pub(crate) applied_offset: u64,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) ansi: String,
    pub(crate) lines: Vec<String>,
}

pub(in crate::vt_worker) struct SessionState {
    pub(in crate::vt_worker) session_id: String,
    pub(in crate::vt_worker) generation: String,
    pub(in crate::vt_worker) scrollback: usize,
    pub(in crate::vt_worker) status: AtomicU8,
    /// Advanced before a resize command can be observed by the actor.
    pub(in crate::vt_worker) geometry_revision: AtomicU64,
    /// Geometry queued to the pipe; only a snapshot confirms it was applied.
    pub(in crate::vt_worker) sent_revision: AtomicU64,
    /// End of the ordered byte stream sent to the worker, not an applied ACK.
    pub(in crate::vt_worker) sent_offset: AtomicU64,
    pub(in crate::vt_worker) sent_cols: AtomicU16,
    pub(in crate::vt_worker) sent_rows: AtomicU16,
    pub(in crate::vt_worker) dirty_bytes: AtomicUsize,
    pub(in crate::vt_worker) snapshot: Mutex<Option<VtSnapshot>>,
    pub(in crate::vt_worker) cached_snapshot_bytes: Arc<AtomicUsize>,
    pub(in crate::vt_worker) pending_write: Mutex<Weak<Mutex<Option<WriteBatch>>>>,
}

impl VtSnapshot {
    pub(in crate::vt_worker) fn cached_bytes(&self) -> usize {
        self.ansi.len() + self.lines.iter().map(String::len).sum::<usize>()
    }
}

impl SessionState {
    pub(in crate::vt_worker) fn is_active(&self) -> bool {
        self.status.load(Ordering::Acquire) == SESSION_ACTIVE
    }

    pub(in crate::vt_worker) fn can_process(&self) -> bool {
        matches!(
            self.status.load(Ordering::Acquire),
            SESSION_ACTIVE | SESSION_DISPOSING
        )
    }

    pub(in crate::vt_worker) fn fail(&self) {
        self.status.store(SESSION_FAILED, Ordering::Release);
        self.clear_snapshot();
    }

    pub(in crate::vt_worker) fn invalidate_for_resize(&self) -> Option<u64> {
        let revision = match self.geometry_revision.try_update(
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

    pub(in crate::vt_worker) fn clear_snapshot(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock()
            && let Some(snapshot) = snapshot.take()
        {
            let previous = self
                .cached_snapshot_bytes
                .fetch_sub(snapshot.cached_bytes(), Ordering::AcqRel);
            debug_assert!(previous >= snapshot.cached_bytes());
        }
    }

    pub(in crate::vt_worker) fn store_snapshot(&self, revision: u64, snapshot: VtSnapshot) -> bool {
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
