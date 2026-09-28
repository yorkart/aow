use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use aow_operation_log::{Level, Outcome, Reader, Record, Writer};
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    Entry, Handle, Inner, LogEntry, LogWorker, Operation, OperationService, Runtime, Snapshot, Spec,
};

impl OperationService {
    pub(crate) fn in_memory() -> Self {
        Self::new(None).expect("in-memory operation service")
    }

    pub(crate) fn persistent(directory: &Path) -> Result<Self, aow_operation_log::Error> {
        Self::new(Some(directory))
    }

    fn new(directory: Option<&Path>) -> Result<Self, aow_operation_log::Error> {
        let mut runtime = Runtime {
            boot_id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            entries: Vec::new(),
            log_error: None,
        };
        let (changes, _) = watch::channel(runtime.snapshot());
        let runtime = Arc::new(Mutex::new(runtime));
        let worker = if let Some(directory) = directory {
            let writer = Writer::open(directory, 24 * 30)?;
            let (sender, mut receiver) = mpsc::channel::<LogEntry>(1024);
            let runtime = runtime.clone();
            let changes = changes.clone();
            let thread = std::thread::Builder::new()
                .name("operation-log".into())
                .spawn(move || {
                    while let Some(entry) = receiver.blocking_recv() {
                        let result = writer.append(&entry.record);
                        let persisted = result.is_ok();
                        if let Err(error) = result {
                            tracing::error!(%error, "operation log write failed");
                            let mut state =
                                runtime.lock().unwrap_or_else(|error| error.into_inner());
                            state.log_error = Some("操作日志写入失败，部分记录可能不完整".into());
                            state.revision += 1;
                            changes.send_replace(state.snapshot());
                        }
                        if let Some(reply) = entry.persisted {
                            let _ = reply.send(persisted);
                        }
                    }
                })?;
            Some(LogWorker {
                sender: Some(sender),
                thread: Some(thread),
            })
        } else {
            None
        };
        Ok(Self {
            inner: Arc::new(Inner {
                runtime,
                changes,
                worker,
                reader: directory.map(Reader::new),
                reads: Arc::new(tokio::sync::Semaphore::new(2)),
            }),
        })
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.inner
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
    }

    /// Standalone audit events do not create live progress operations. Wait for
    /// the bounded queue and disk write instead of dropping events under load.
    pub(crate) async fn record(&self, mut record: Record) {
        record.boot_id = self.inner.runtime.lock().unwrap().boot_id.clone();
        let Some(worker) = &self.inner.worker else {
            return;
        };
        let (reply, persisted) = oneshot::channel();
        let event_id = record.operation_id.clone();
        if worker
            .sender
            .as_ref()
            .unwrap()
            .send(LogEntry {
                record,
                persisted: Some(reply),
            })
            .await
            .is_err()
            || !persisted.await.unwrap_or(false)
        {
            tracing::error!(%event_id, "audit log persistence failed");
            let mut state = self.inner.runtime.lock().unwrap();
            state.log_error = Some("审计日志写入失败，部分记录可能不完整".into());
            state.revision += 1;
            self.inner.changes.send_replace(state.snapshot());
        } else {
            let mut state = self.inner.runtime.lock().unwrap();
            state.revision += 1;
            self.inner.changes.send_replace(state.snapshot());
        }
    }

    pub(crate) fn begin(&self, spec: Spec) -> Handle {
        let operation = Operation {
            id: spec.id.clone(),
            kind: spec.kind.into(),
            source: spec.source.into(),
            title: bounded(&spec.title, 256),
            project_id: spec.project_id.map(|value| bounded(&value, 256)),
            resource: spec.resource.map(|value| bounded(&value, 4096)),
            created_at: chrono::Utc::now().to_rfc3339(),
            message: "排队中".into(),
            completed: 0,
            total: spec.total,
            outcome: None,
        };
        {
            let mut state = self
                .inner
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.entries.push(Entry {
                operation,
                finished: None,
            });
        }
        self.update(
            &spec.id,
            "started",
            Level::Info,
            "操作已接受".into(),
            None,
            None,
            None,
        );
        Handle::new(self.clone(), spec.id)
    }

    pub(super) fn update(
        &self,
        id: &str,
        event: &str,
        level: Level,
        message: String,
        completed: Option<usize>,
        outcome: Option<Outcome>,
        resource: Option<String>,
    ) {
        let mut state = self
            .inner
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let boot_id = state.boot_id.clone();
        let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.operation.id == id)
        else {
            return;
        };
        // A terminal outcome is immutable; late worker callbacks cannot revive it.
        if entry.finished.is_some() {
            return;
        }
        let operation = &mut entry.operation;
        operation.message = bounded(&message, 8192);
        if let Some(completed) = completed {
            operation.completed = completed;
        }
        if let Some(resource) = resource {
            operation.resource = Some(bounded(&resource, 4096));
        }
        operation.outcome = outcome;
        if outcome.is_some() {
            entry.finished = Some(Instant::now());
        }
        let record = Record {
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation_id: id.into(),
            boot_id,
            kind: operation.kind.clone(),
            source: operation.source.clone(),
            title: operation.title.clone(),
            event: event.into(),
            level,
            message: operation.message.clone(),
            project_id: operation.project_id.clone(),
            resource: operation.resource.clone(),
            outcome,
            completed: Some(operation.completed),
            total: operation.total,
        };
        if let Some(worker) = &self.inner.worker
            && worker
                .sender
                .as_ref()
                .unwrap()
                .try_send(LogEntry {
                    record,
                    persisted: None,
                })
                .is_err()
        {
            state.log_error = Some("操作日志队列已满或不可用，部分记录可能不完整".into());
            tracing::error!(operation_id = id, "operation log queue unavailable");
        }
        state.revision += 1;
        self.inner.changes.send_replace(state.snapshot());
    }
}

fn bounded(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.into();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
