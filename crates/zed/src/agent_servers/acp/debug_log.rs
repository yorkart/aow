//! Zed AcpDebugLog boundary: ordered wire messages and stderr, bounded in memory.
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct AcpDebugLog(Arc<Mutex<VecDeque<Value>>>);
impl AcpDebugLog {
    pub(crate) fn record_line(&self, direction: &str, line: &str) {
        let mut messages = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if messages.len() >= 500 {
            messages.pop_front();
        }
        messages.push_back(json!({"direction":direction,"timestamp":chrono::Utc::now().to_rfc3339(),"message":line.chars().take(65536).collect::<String>()}));
    }
    pub(crate) fn messages(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}
