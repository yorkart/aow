//! Service-wide retention. Connection handles do not own separate message buffers.
use super::{AcpDebugLog, LogState};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

pub(super) const MAX_ARCHIVED_CONNECTIONS: usize = 64;
pub(super) const MAX_MESSAGES: usize = 10_000;
pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) struct AcpDebugLogStore {
    pub(super) state: Arc<Mutex<StoreState>>,
}

#[derive(Default)]
pub(super) struct StoreState {
    pub(super) connections: HashMap<String, LogState>,
    archived: VecDeque<String>,
    sequence: u64,
}

impl AcpDebugLogStore {
    pub(crate) fn create_log(&self) -> AcpDebugLog {
        let connection_id = uuid::Uuid::new_v4().to_string();
        self.state.lock().unwrap().connections.insert(
            connection_id.clone(),
            LogState {
                active: true,
                ..Default::default()
            },
        );
        AcpDebugLog {
            connection_id,
            store: self.clone(),
        }
    }

    pub(crate) fn messages(&self, connection_id: &str) -> Option<Vec<Value>> {
        self.state
            .lock()
            .unwrap()
            .connections
            .get(connection_id)
            .map(|log| {
                log.messages
                    .iter()
                    .map(|entry| entry.value.clone())
                    .collect()
            })
    }

    pub(crate) fn thread_messages(&self, thread_id: &str) -> Vec<Value> {
        self.state
            .lock()
            .unwrap()
            .connections
            .values()
            .filter(|log| log.threads.contains(thread_id))
            .flat_map(|log| log.messages.iter())
            .filter(|entry| {
                entry.value["thread_id"] == thread_id || entry.value["scope"] == "connection"
            })
            .map(|entry| entry.value.clone())
            .collect()
    }

    pub(crate) fn remove_connection(&self, id: &str) {
        let mut store = self.state.lock().unwrap();
        store.connections.remove(id);
        store.archived.retain(|archived| archived != id);
    }

    pub(crate) fn remove_thread(&self, thread_id: &str) {
        let mut store = self.state.lock().unwrap();
        store.connections.retain(|_, log| {
            if !log.threads.remove(thread_id) {
                return true;
            }
            log.sessions.retain(|_, id| id != thread_id);
            log.requests
                .retain(|context| context.thread_id.as_deref() != Some(thread_id));
            log.elicitations
                .retain(|context| context.thread_id.as_deref() != Some(thread_id));
            log.messages
                .retain(|entry| entry.value["thread_id"] != thread_id);
            if log.threads.is_empty() {
                // Keep an active handle usable for future sessions, with no deleted history.
                log.messages.clear();
                log.requests.clear();
                log.elicitations.clear();
            }
            log.bytes = log.messages.iter().map(|entry| entry.bytes).sum();
            log.active || !log.threads.is_empty()
        });
        let StoreState {
            connections,
            archived,
            ..
        } = &mut *store;
        archived.retain(|id| connections.contains_key(id));
    }
}

impl StoreState {
    pub(super) fn update<R>(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut LogState, u64) -> R,
    ) -> Option<R> {
        self.sequence += 1;
        let result = apply(self.connections.get_mut(id)?, self.sequence);
        self.enforce_limits();
        Some(result)
    }

    pub(super) fn archive(&mut self, id: &str) {
        let Some(log) = self.connections.get_mut(id) else {
            return;
        };
        if !log.active {
            return;
        }
        log.active = false;
        log.requests.clear();
        log.elicitations.clear();
        log.sessions.clear();
        self.archived.push_back(id.into());
        while self.archived.len() > MAX_ARCHIVED_CONNECTIONS {
            if let Some(id) = self.archived.pop_front() {
                self.connections.remove(&id);
            }
        }
    }

    fn enforce_limits(&mut self) {
        let mut bytes: usize = self.connections.values().map(|log| log.bytes).sum();
        let mut messages: usize = self
            .connections
            .values()
            .map(|log| log.messages.len())
            .sum();
        while bytes > MAX_BYTES || messages > MAX_MESSAGES {
            let Some(log) = self
                .connections
                .values_mut()
                .filter(|log| !log.messages.is_empty())
                .min_by_key(|log| log.messages.front().unwrap().sequence)
            else {
                break;
            };
            let entry = log.messages.pop_front().unwrap();
            log.bytes -= entry.bytes;
            bytes -= entry.bytes;
            messages -= 1;
        }
    }
}
