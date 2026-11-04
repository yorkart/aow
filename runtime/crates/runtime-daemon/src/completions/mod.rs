//! Execution-node completion collection and replayable transport.
mod collector;
mod registry;
mod storage;
#[cfg(test)]
mod tests;
mod transport;

use anyhow::Result;
use axum::{Router, routing::get};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone, Debug, Serialize)]
struct Completion {
    agent: String,
    session_id: String,
    title: String,
    cwd: String,
    turn_id: Option<String>,
    conclusion: Option<String>,
    usage: Option<aow_agents::sessions::usage::TokenUsage>,
    instance_ids: Vec<String>,
}
#[derive(Clone)]
pub(crate) struct Completions {
    inner: Arc<Inner>,
}
struct Inner {
    node_id: String,
    log: Mutex<rusqlite::Connection>,
    changed: watch::Sender<u64>,
    stopping: watch::Receiver<bool>,
}
impl Completions {
    pub(crate) fn open(
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
    pub(crate) fn router(&self) -> Router {
        Router::new()
            .route("/v1/agent-events", get(transport::events))
            .with_state(self.clone())
    }
    fn append(&self, events: &[Completion]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let head = storage::append(&mut self.inner.log.lock().unwrap(), events)?;
        self.inner.changed.send_replace(head);
        Ok(())
    }
}
