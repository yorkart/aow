//! Shared live operations and file-backed history. Business workers own their
//! execution; history is never used to replay or reconstruct a worker.

use aow_operation_log::{Outcome, Reader, Record};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

mod api;
mod handle;
mod service;

#[cfg(test)]
mod tests;

pub(crate) use api::routes;
pub(crate) use handle::Handle;

const RECENT_TTL: Duration = Duration::from_secs(300);
const RECENT_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Operation {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub project_id: Option<String>,
    pub resource: Option<String>,
    pub created_at: String,
    pub message: String,
    pub completed: usize,
    pub total: Option<usize>,
    pub outcome: Option<Outcome>,
}

pub(crate) struct Spec {
    pub id: String,
    pub kind: &'static str,
    pub source: &'static str,
    pub title: String,
    pub project_id: Option<String>,
    pub resource: Option<String>,
    pub total: Option<usize>,
}

#[derive(Clone, Default, Serialize)]
pub(crate) struct Snapshot {
    pub boot_id: String,
    pub revision: u64,
    pub operations: Vec<Operation>,
    pub log_error: Option<String>,
}

struct Entry {
    operation: Operation,
    finished: Option<Instant>,
}
struct Runtime {
    boot_id: String,
    revision: u64,
    entries: Vec<Entry>,
    log_error: Option<String>,
}

impl Runtime {
    fn snapshot(&mut self) -> Snapshot {
        self.entries
            .retain(|entry| entry.finished.is_none_or(|at| at.elapsed() < RECENT_TTL));
        // Retain by completion time, not creation order: a long-running task
        // must still publish its result after many newer tasks have completed.
        while self
            .entries
            .iter()
            .filter(|entry| entry.finished.is_some())
            .count()
            > RECENT_LIMIT
        {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.finished.map(|at| (index, at)))
                .min_by_key(|(_, at)| *at)
                .unwrap()
                .0;
            self.entries.remove(oldest);
        }
        Snapshot {
            boot_id: self.boot_id.clone(),
            revision: self.revision,
            operations: self
                .entries
                .iter()
                .map(|entry| entry.operation.clone())
                .collect(),
            log_error: self.log_error.clone(),
        }
    }
}

struct LogWorker {
    sender: Option<mpsc::Sender<LogEntry>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct LogEntry {
    record: Record,
    persisted: Option<oneshot::Sender<bool>>,
}
impl Drop for LogWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Inner {
    runtime: Arc<Mutex<Runtime>>,
    changes: watch::Sender<Snapshot>,
    worker: Option<LogWorker>,
    reader: Option<Reader>,
    reads: Arc<tokio::sync::Semaphore>,
}

#[derive(Clone)]
pub(crate) struct OperationService {
    inner: Arc<Inner>,
}
