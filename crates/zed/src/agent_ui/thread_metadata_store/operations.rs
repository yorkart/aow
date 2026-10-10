//! thread_metadata_store.rs::DbOperation and its background drain/dedup loop.
//! A dedicated thread replaces GPUI's background executor; snapshots join the same transaction.
use super::{ThreadMetadata, ThreadMetadataStore, db::ThreadMetadataDb};
use crate::{acp_thread::snapshot_store::SnapshotDb, facade::SessionSnapshot};
use anyhow::{Result, anyhow};
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) enum DbOperation {
    // The second payload is AoW's offline display cache, not upstream metadata.
    Upsert(Box<ThreadMetadata>, Box<SessionSnapshot>),
    Delete(String),
}
impl DbOperation {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Upsert(thread, _) => &thread.thread_id,
            Self::Delete(id) => id,
        }
    }
}
#[derive(Default)]
struct Pending {
    // Coalesce at enqueue too: an unavailable disk cannot grow an unbounded stream queue.
    operations: HashMap<String, DbOperation>,
    waiters: Vec<mpsc::Sender<Result<(), String>>>,
    shutdown: bool,
}
pub(super) struct DbOperations {
    pending: Arc<(Mutex<Pending>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}
impl DbOperations {
    pub(super) fn new(db: ThreadMetadataDb) -> Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let state = pending.clone();
        let worker = std::thread::Builder::new()
            .name("acp-thread-db".into())
            .spawn(move || Self::run(db, state))?;
        Ok(Self {
            pending,
            worker: Some(worker),
        })
    }
    pub(super) fn enqueue(&self, operation: DbOperation) {
        let (lock, wake) = &*self.pending;
        lock.lock()
            .unwrap()
            .operations
            .insert(operation.id().to_owned(), operation);
        wake.notify_one();
    }
    pub(super) fn flush(&self) -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let (lock, wake) = &*self.pending;
        lock.lock().unwrap().waiters.push(sender);
        wake.notify_one();
        receiver
            .recv()
            .map_err(|_| anyhow!("ACP database writer stopped"))?
            .map_err(anyhow::Error::msg)
    }
    fn run(db: ThreadMetadataDb, pending: Arc<(Mutex<Pending>, Condvar)>) {
        let (lock, wake) = &*pending;
        loop {
            let mut state = lock.lock().unwrap();
            while state.operations.is_empty() && state.waiters.is_empty() && !state.shutdown {
                state = wake.wait(state).unwrap();
            }
            // A fixed deadline avoids both per-chunk fsync and starvation under continuous output.
            let deadline = Instant::now() + Duration::from_millis(100);
            while state.waiters.is_empty() && !state.shutdown {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                state = wake.wait_timeout(state, remaining).unwrap().0;
            }
            let operations = ThreadMetadataStore::dedup_db_operations(
                std::mem::take(&mut state.operations)
                    .into_values()
                    .collect(),
            );
            let waiters = std::mem::take(&mut state.waiters);
            let shutdown = state.shutdown;
            drop(state);
            let result = Self::write(&db, &operations)
                .map_err(|error| format!("Cannot persist ACP history: {error:#}"));
            if let Err(error) = &result {
                tracing::error!(%error, "ACP database write failed; pending state retained for retry");
                let mut state = lock.lock().unwrap();
                for operation in operations {
                    // A newer update/delete queued during I/O always wins over this failed write.
                    state
                        .operations
                        .entry(operation.id().to_owned())
                        .or_insert(operation);
                }
            }
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
            if shutdown {
                break;
            }
        }
    }
    fn write(db: &ThreadMetadataDb, operations: &[DbOperation]) -> Result<()> {
        if operations.is_empty() {
            return Ok(());
        }
        let mut connection = db.0.lock().unwrap();
        let transaction = connection.transaction()?;
        for operation in operations {
            match operation {
                DbOperation::Upsert(metadata, snapshot) => {
                    ThreadMetadataDb::save(&transaction, metadata)?;
                    SnapshotDb::save_thread(&transaction, snapshot)?;
                }
                DbOperation::Delete(id) => {
                    SnapshotDb::delete_thread(&transaction, id)?;
                    ThreadMetadataDb::delete(&transaction, id)?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
impl Drop for DbOperations {
    fn drop(&mut self) {
        let (lock, wake) = &*self.pending;
        lock.lock().unwrap().shutdown = true;
        wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
