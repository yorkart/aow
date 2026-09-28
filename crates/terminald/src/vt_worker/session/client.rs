use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU8, AtomicU16, AtomicU64, AtomicUsize},
};

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use super::{
    super::config::{SESSION_ACTIVE, normalize_vt_cols},
    super::queue::WorkerCommand,
    handle::VtSession,
    state::SessionState,
};

#[derive(Clone)]
pub(crate) struct VtWorkerClient {
    pub(in crate::vt_worker) sender: mpsc::Sender<WorkerCommand>,
    pub(in crate::vt_worker) shutdown: Arc<watch::Sender<bool>>,
    pub(in crate::vt_worker) scrollback: usize,
    pub(in crate::vt_worker) sessions: Arc<Mutex<Vec<Weak<SessionState>>>>,
    pub(in crate::vt_worker) cached_snapshot_bytes: Arc<AtomicUsize>,
    pub(in crate::vt_worker) pending_write_bytes: Arc<AtomicUsize>,
}

impl VtWorkerClient {
    pub(in crate::vt_worker) fn shutdown_now(&self) {
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
