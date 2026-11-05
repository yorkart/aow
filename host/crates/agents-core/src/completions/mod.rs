//! Native completion collection, durable cursors and replay.
mod collector;
mod registry;
mod storage;
#[cfg(test)]
mod tests;

use anyhow::Result;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone, Debug, Serialize)]
pub struct Completion {
    pub agent: String,
    pub agent_display_name: String,
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub turn_id: Option<String>,
    pub conclusion: Option<String>,
    pub usage: Option<crate::sessions::usage::TokenUsage>,
    pub instance_ids: Vec<String>,
}
#[derive(Clone)]
pub struct Completions {
    inner: Arc<Inner>,
}
struct Inner {
    node_id: String,
    log: Mutex<rusqlite::Connection>,
    changed: watch::Sender<u64>,
    stopping: watch::Receiver<bool>,
}
impl Completions {
    pub fn open(
        root: &std::path::Path,
        node_id: String,
        stopping: watch::Receiver<bool>,
    ) -> Result<Self> {
        let log = storage::open(root)?;
        let (_, head) = storage::bounds(&log)?;
        Ok(Self {
            inner: Arc::new(Inner {
                node_id,
                log: Mutex::new(log),
                changed: watch::channel(head).0,
                stopping,
            }),
        })
    }
    pub fn node_id(&self) -> &str {
        &self.inner.node_id
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.changed.subscribe()
    }
    pub fn stopping(&self) -> watch::Receiver<bool> {
        self.inner.stopping.clone()
    }
    pub fn bounds(&self) -> Result<(u64, u64)> {
        storage::bounds(&self.inner.log.lock().unwrap())
    }
    pub fn read(&self, after: u64) -> Result<Vec<serde_json::Value>> {
        storage::read(&self.inner.log.lock().unwrap(), after)
    }

    pub fn append(&self, events: &[Completion]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let head = storage::append(&mut self.inner.log.lock().unwrap(), events)?;
        self.inner.changed.send_replace(head);
        Ok(())
    }
}
