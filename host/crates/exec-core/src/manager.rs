//! Durable jobs with idempotent admission. No timers, retries or task business records.
use crate::{
    ExecutionSpec, RunEvent, RunStatus, Store, store::valid_id, tracking::ConcurrencySlot,
};
use anyhow::{Result, ensure};
use aow_host_contract::execution::{ExecutionEvent, ExecutionResult, ExecutionSnapshot};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
#[derive(Clone)]
pub struct ExecutionManager(Arc<Inner>);
struct Inner {
    store: Store,
    jobs: Mutex<HashMap<String, Job>>,
    stopping: watch::Receiver<bool>,
    _ownership: Option<Arc<dyn Send + Sync>>,
    receipts: Mutex<rusqlite::Connection>,
}
struct Job {
    cancel: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    capacity: Option<crate::CapacityReservation>,
}
impl ExecutionManager {
    pub fn open(root: &Path, stopping: watch::Receiver<bool>) -> Result<Self> {
        let store = Store::new(root)?;
        let receipts = rusqlite::Connection::open(store.root.join("receipts.sqlite"))?;
        receipts.busy_timeout(Duration::from_secs(5))?;
        receipts.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY,status TEXT NOT NULL)")?;
        let service = Self(Arc::new(Inner {
            store,
            receipts: Mutex::new(receipts),
            jobs: Mutex::default(),
            stopping,
            _ownership: None,
        }));
        for entry in std::fs::read_dir(&service.0.store.root)? {
            let path = entry?.path();
            if !path.is_dir() || !path.join("spec.json").is_file() {
                continue;
            }
            let id = path.file_name().unwrap().to_string_lossy();
            valid_id(&id)?;
            if !service.snapshot(&id, 0)?.terminal {
                ConcurrencySlot::recover(&path.join("owner.lock"))?;
                service.0.store.finish(
                    &id,
                    RunStatus::Interrupted,
                    "Execution node restarted before completion",
                )?;
            }
        }
        Ok(service)
    }
    pub fn with_ownership(mut self, ownership: Arc<dyn Send + Sync>) -> Self {
        Arc::get_mut(&mut self.0)
            .expect("ownership configured before execution admission")
            ._ownership = Some(ownership);
        self
    }
    pub fn submit(&self, spec: ExecutionSpec) -> Result<()> {
        ensure!(!*self.0.stopping.borrow(), "Execution node is stopping");
        valid_id(&spec.id)?;
        ensure!(
            spec.repository_path.is_absolute() && spec.launch.executable.is_absolute(),
            "Execution paths must be absolute on the node"
        );
        ensure!(
            spec.input.prompt.len() <= 8 * 1024 * 1024,
            "Execution prompt exceeds limit"
        );
        spec.input
            .workspace
            .validate()
            .map_err(|error| anyhow::anyhow!(aow_i18n::text(error.0)))?;
        use aow_agent_runtime_core::automation::AgentAutomation;
        spec.input.agent.validate_arguments(&spec.launch.args)?;
        if let Some(capacity) = &spec.capacity {
            valid_id(&capacity.key)?;
            ensure!(
                (1..=10).contains(&capacity.limit),
                "Invalid execution capacity"
            );
        }
        let mut jobs = self.0.jobs.lock().unwrap();
        ensure!(!*self.0.stopping.borrow(), "Execution node is stopping");
        let path = self.0.store.run_path("", &spec.id)?;
        ensure!(
            self.receipt(&spec.id)?.is_none(),
            "Execution has been retired"
        );
        if path.exists() {
            let existing: Value = serde_json::from_slice(&std::fs::read(path.join("spec.json"))?)?;
            ensure!(
                existing == serde_json::to_value(&spec)?,
                "Execution ID already belongs to another specification"
            );
            return Ok(());
        }
        ensure!(jobs.len() < 256, "Execution node is at capacity");
        if let Some(capacity) = &spec.capacity {
            ensure!(
                jobs.values()
                    .filter(|job| job
                        .capacity
                        .as_ref()
                        .is_some_and(|other| other.key == capacity.key))
                    .count()
                    < capacity.limit as usize,
                "Execution reservation is busy"
            );
        }
        let writer = self.0.store.create(&spec)?;
        let id = spec.id.clone();
        let capacity = spec.capacity.clone();
        let (cancel, stopping) = watch::channel(false);
        let service = self.clone();
        let job_id = id.clone();
        let task = aow_i18n::spawn(async move {
            if let Err(error) =
                crate::runner::execute(service.0.store.clone(), spec, writer, stopping).await
            {
                tracing::error!(%error,id=%job_id,"execution failed");
                if let Err(error) =
                    service
                        .0
                        .store
                        .finish(&job_id, RunStatus::Failed, &format!("{error:#}"))
                {
                    tracing::error!(%error,id=%job_id,"cannot finalize execution journal");
                }
            }
            service.0.jobs.lock().unwrap().remove(&job_id);
        });
        jobs.insert(
            id,
            Job {
                cancel,
                task,
                capacity,
            },
        );
        Ok(())
    }
    fn receipt(&self, id: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .receipts
            .lock()
            .unwrap()
            .query_row("SELECT status FROM receipts WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }
    fn record_receipt(&self, id: &str, status: &str) -> Result<()> {
        self.0.receipts.lock().unwrap().execute("INSERT INTO receipts(id,status) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET status=excluded.status",rusqlite::params![id,if status == "cancelled" { format!("cancelled|{}",chrono::Utc::now().to_rfc3339()) } else { status.to_owned() }])?;
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let jobs = self.0.jobs.lock().unwrap();
        if let Some(job) = jobs.get(id) {
            job.cancel.send_replace(true);
        } else if !self.0.store.run_path("", id)?.exists() && self.receipt(id)?.is_none() {
            self.record_receipt(id, "cancelled")?;
        }
        Ok(())
    }
    pub fn snapshot(&self, id: &str, after: u64) -> Result<ExecutionSnapshot> {
        valid_id(id)?;
        if let Some(receipt) = self.receipt(id)? {
            if let Some(at) = receipt.strip_prefix("cancelled|") {
                ensure!(after <= 1, "Execution cursor is ahead of journal");
                let event = RunEvent::Finished {
                    at: chrono::DateTime::parse_from_rfc3339(at)?.with_timezone(&chrono::Utc),
                    status: RunStatus::Interrupted,
                    exit_code: None,
                    message: Some("Execution was cancelled before admission".into()),
                    duration_ms: 0,
                };
                return Ok(ExecutionSnapshot {
                    id: id.into(),
                    node_id: String::new(),
                    head: 1,
                    terminal: true,
                    events: if after == 0 {
                        vec![ExecutionEvent { sequence: 1, event }]
                    } else {
                        Vec::new()
                    },
                    result: None,
                    native_session: None,
                });
            }
            anyhow::bail!("Execution has been retired");
        }
        let directory = self.0.store.run_path("", id)?;
        let bytes = std::fs::read(directory.join("events.jsonl"))?;
        ensure!(
            bytes.len() <= 1024 * 1024,
            "Execution journal exceeds its limit"
        );
        let mut events = Vec::new();
        let mut terminal = false;
        let mut sequence = 0_u64;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            if !line.ends_with(b"\n") {
                break;
            }
            let event: RunEvent = serde_json::from_slice(line)?;
            sequence += 1;
            terminal = matches!(event, RunEvent::Finished { .. });
            if sequence > after {
                events.push(ExecutionEvent { sequence, event });
            }
        }
        ensure!(after <= sequence, "Execution cursor is ahead of journal");
        let result = match std::fs::read(directory.join("result.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<ExecutionResult>(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(ExecutionSnapshot {
            id: id.into(),
            node_id: String::new(),
            head: sequence,
            terminal,
            events,
            result,
            native_session: None,
        })
    }
    pub fn output(&self, id: &str, output: &str, offset: u64) -> Result<Vec<u8>> {
        ensure!(
            matches!(output, "stdio" | "stderr"),
            "Invalid execution output"
        );
        if self
            .receipt(id)?
            .is_some_and(|value| value.starts_with("cancelled|"))
        {
            return Ok(Vec::new());
        }
        let mut file = std::fs::File::open(self.0.store.run_path("", id)?.join(output))?;
        ensure!(
            offset <= file.metadata()?.len(),
            "Output cursor is ahead of file"
        );
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    pub fn native_session(
        &self,
        id: &str,
    ) -> Result<Option<aow_agent_runtime_core::sessions::AgentSession>> {
        let directory = self.0.store.run_path("", id)?;
        let spec: ExecutionSpec =
            serde_json::from_slice(&std::fs::read(directory.join("spec.json"))?)?;
        let snapshot = self.snapshot(id, 0)?;
        let session = snapshot
            .events
            .iter()
            .rev()
            .find_map(|item| match &item.event {
                RunEvent::Session { session_id, .. } => Some(session_id.as_str()),
                _ => None,
            });
        let Some(session) = session else {
            return Ok(None);
        };
        let roots = self.0.store.run_session_roots("", id)?;
        Ok(aow_agent_runtime_core::sessions::find_session(
            spec.input.agent.id(),
            session,
            roots,
        ))
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let jobs = self.0.jobs.lock().unwrap();
        ensure!(!jobs.contains_key(id), "Execution is still active");
        let path = self.0.store.run_path("", id)?;
        if path.exists() {
            ensure!(
                self.snapshot(id, 0)?.terminal,
                "Execution has not completed"
            );
        }
        self.record_receipt(id, "removed")?;
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    pub async fn shutdown(&self) -> Result<()> {
        let jobs = std::mem::take(&mut *self.0.jobs.lock().unwrap());
        for job in jobs.values() {
            job.cancel.send_replace(true);
        }
        let mut jobs: Vec<_> = jobs.into_iter().collect();
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            for (_, job) in &mut jobs {
                let _ = (&mut job.task).await;
            }
        })
        .await;
        if result.is_err() {
            for (_, job) in &jobs {
                job.task.abort();
            }
            for (id, mut job) in jobs {
                if !job.task.is_finished() {
                    let _ = (&mut job.task).await;
                }
                self.0.store.finish(
                    &id,
                    RunStatus::Interrupted,
                    "Execution was stopped during node shutdown",
                )?;
            }
        }
        Ok(())
    }
}
impl Store {
    fn finish(&self, id: &str, status: RunStatus, message: &str) -> Result<()> {
        let path = self.run_path("", id)?.join("events.jsonl");
        let bytes = std::fs::read(&path)?;
        let end = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        if bytes[..end]
            .split_inclusive(|byte| *byte == b'\n')
            .next_back()
            .is_some_and(|line| {
                serde_json::from_slice::<Value>(line).is_ok_and(|value| value["type"] == "finished")
            })
        {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        file.set_len(end as u64)?;
        use std::io::Write;
        writeln!(
            file,
            "{}",
            serde_json::to_string(&RunEvent::Finished {
                at: chrono::Utc::now(),
                status,
                exit_code: None,
                message: Some(message.into()),
                duration_ms: 0
            })?
        )?;
        file.sync_all()?;
        Ok(())
    }
}
