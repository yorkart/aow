use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use super::{
    config::{MAX_PENDING_WRITE_BYTES, MAX_WRITE_BATCH_BYTES},
    session::SessionState,
};

pub(super) struct WriteBatch {
    pub(super) revision: u64,
    pub(super) start_offset: u64,
    pub(super) data: Vec<u8>,
    pub(super) pending_bytes: Arc<AtomicUsize>,
}

impl WriteBatch {
    pub(super) fn new(
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

    pub(super) fn append(&mut self, revision: u64, start_offset: u64, data: &[u8]) -> bool {
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

pub(super) fn reserve_write_bytes(total: &AtomicUsize, bytes: usize) -> bool {
    total
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current
                .checked_add(bytes)
                .filter(|next| *next <= MAX_PENDING_WRITE_BYTES)
        })
        .is_ok()
}
pub(super) enum WorkerCommand {
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

impl WorkerCommand {
    pub(super) fn session(&self) -> &Arc<SessionState> {
        match self {
            Self::Create { session, .. }
            | Self::Write { session, .. }
            | Self::Resize { session, .. }
            | Self::Dispose { session } => session,
        }
    }

    pub(super) fn kind(&self) -> WorkerCommandKind {
        match self {
            Self::Create { .. } => WorkerCommandKind::Create,
            Self::Write { .. } => WorkerCommandKind::Write,
            Self::Resize { .. } => WorkerCommandKind::Resize,
            Self::Dispose { .. } => WorkerCommandKind::Dispose,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum WorkerCommandKind {
    Create,
    Write,
    Resize,
    Dispose,
}
