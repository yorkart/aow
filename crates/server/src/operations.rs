//! Shared live operations and file-backed history. Business workers own their
//! execution; history is never used to replay or reconstruct a worker.

use crate::{AppState, HttpError};
use aow_operation_log::{
    Cursor, Filter, Level, Outcome, Page, ReadOptions, Reader, Record, Writer,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[cfg(test)]
mod tests;

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
    sender: Option<mpsc::SyncSender<Record>>,
    thread: Option<std::thread::JoinHandle<()>>,
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
            let (sender, receiver) = mpsc::sync_channel::<Record>(1024);
            let runtime = runtime.clone();
            let changes = changes.clone();
            let thread = std::thread::Builder::new()
                .name("operation-log".into())
                .spawn(move || {
                    while let Ok(record) = receiver.recv() {
                        if let Err(error) = writer.append(&record) {
                            tracing::error!(%error, "operation log write failed");
                            let mut state =
                                runtime.lock().unwrap_or_else(|error| error.into_inner());
                            state.log_error = Some("操作日志写入失败，部分记录可能不完整".into());
                            state.revision += 1;
                            changes.send_replace(state.snapshot());
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
        Handle {
            inner: Arc::new(HandleInner {
                service: self.clone(),
                id: spec.id,
            }),
        }
    }

    fn update(
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
            && worker.sender.as_ref().unwrap().try_send(record).is_err()
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

struct HandleInner {
    service: OperationService,
    id: String,
}
impl Drop for HandleInner {
    fn drop(&mut self) {
        self.service.update(
            &self.id,
            "finished",
            Level::Warn,
            "执行过程已中断，结果未确认；请检查资源状态".into(),
            None,
            Some(Outcome::Interrupted),
            None,
        );
    }
}

#[derive(Clone)]
pub(crate) struct Handle {
    inner: Arc<HandleInner>,
}
impl Handle {
    pub(crate) fn progress(&self, message: impl Into<String>, completed: Option<usize>) {
        self.inner.service.update(
            &self.inner.id,
            "progress",
            Level::Info,
            message.into(),
            completed,
            None,
            None,
        );
    }
    pub(crate) fn event(&self, level: Level, message: impl Into<String>, completed: Option<usize>) {
        self.inner.service.update(
            &self.inner.id,
            "step",
            level,
            message.into(),
            completed,
            None,
            None,
        );
    }
    pub(crate) fn resource(&self, resource: String) {
        self.inner.service.update(
            &self.inner.id,
            "resource",
            Level::Info,
            "关联资源已更新".into(),
            None,
            None,
            Some(resource),
        );
    }
    pub(crate) fn finish(&self, outcome: Outcome, message: impl Into<String>) {
        let level = match outcome {
            Outcome::Succeeded => Level::Info,
            Outcome::Failed => Level::Error,
            _ => Level::Warn,
        };
        self.inner.service.update(
            &self.inner.id,
            "finished",
            level,
            message.into(),
            None,
            Some(outcome),
            None,
        );
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/operations/active", get(active))
        .route("/api/operations/stream", get(stream))
        .route("/api/operation-logs", get(logs))
}

async fn active(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.operations.snapshot())
}

async fn stream(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.operations.inner.changes.subscribe();
    // Subscribe before the snapshot. A reconnect always starts with a complete
    // snapshot, so dropped intermediate progress does not leave stale tasks.
    let stream =
        futures_util::stream::unfold((receiver, true), |(mut receiver, initial)| async move {
            if !initial && receiver.changed().await.is_err() {
                return None;
            }
            let snapshot = receiver.borrow_and_update().clone();
            let event = Event::default()
                .event("operations")
                .json_data(snapshot)
                .expect("serializable snapshot");
            Some((Ok(event), (receiver, false)))
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize, Default)]
struct LogQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    query: Option<String>,
    operation_id: Option<String>,
    kind: Option<String>,
    source: Option<String>,
    project_id: Option<String>,
    level: Option<Level>,
}

async fn logs(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Page>, HttpError> {
    let cursor = query
        .cursor
        .as_ref()
        .map(|cursor| {
            if cursor.len() > 512 {
                return Err(aow_operation_log::Error::InvalidCursor);
            }
            serde_json::from_str::<Cursor>(cursor)
                .map_err(|_| aow_operation_log::Error::InvalidCursor)
        })
        .transpose()
        .map_err(log_error)?;
    let options = ReadOptions {
        cursor,
        limit: query.limit.unwrap_or(50),
        filter: Filter {
            query: query.query,
            operation_id: query.operation_id,
            kind: query.kind,
            source: query.source,
            project_id: query.project_id,
            level: query.level,
        },
        ..Default::default()
    };
    let Some(reader) = state.operations.inner.reader.clone() else {
        return Ok(Json(Page::default()));
    };
    let permit = state
        .operations
        .inner
        .reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "log_queries_busy",
                "日志查询繁忙，请稍后重试",
                None,
            )
        })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        reader.read(&options)
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
    .map(Json)
    .map_err(log_error)
}

fn log_error(error: aow_operation_log::Error) -> HttpError {
    use aow_operation_log::Error;
    let (status, code) = match error {
        Error::InvalidCursor | Error::InvalidOptions => {
            (StatusCode::BAD_REQUEST, "invalid_log_query")
        }
        Error::CursorExpired => (StatusCode::GONE, "log_cursor_expired"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "log_read_failed"),
    };
    HttpError::new(status, code, error.to_string(), None)
}
