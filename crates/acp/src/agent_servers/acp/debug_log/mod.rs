//! Bounded wire logs with AoW thread correlation; shared connection events stay shared.
mod contexts;
mod store;
#[cfg(test)]
mod tests;

use agent_client_protocol::SentRequest;
use contexts::Contexts;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
pub(crate) use store::AcpDebugLogStore;

#[derive(Clone)]
pub(crate) struct AcpDebugLog {
    pub(crate) connection_id: String,
    store: AcpDebugLogStore,
}

#[derive(Default)]
struct LogState {
    active: bool,
    messages: VecDeque<LogMessage>,
    bytes: usize,
    threads: HashSet<String>,
    sessions: HashMap<String, String>,
    // JSON-RPC IDs are scoped to a connection AND to the initiating direction.
    requests: Contexts<(String, String)>,
    elicitations: Contexts<String>,
}

struct LogMessage {
    value: Value,
    bytes: usize,
    sequence: u64,
}

#[derive(Clone, Default)]
struct RequestContext {
    thread_id: Option<String>,
    session_id: Option<String>,
    method: String,
    shared: bool,
}

impl Default for AcpDebugLog {
    fn default() -> Self {
        AcpDebugLogStore::default().create_log()
    }
}

impl AcpDebugLog {
    pub(crate) fn attach(&self, thread_id: &str) {
        self.store
            .state
            .lock()
            .unwrap()
            .update(&self.connection_id, |state, _| {
                state.threads.insert(thread_id.into());
            });
    }

    pub(crate) fn bind_session(&self, session_id: &str, thread_id: &str) {
        self.store
            .state
            .lock()
            .unwrap()
            .update(&self.connection_id, |state, _| {
                state.bind_session(session_id, thread_id);
            });
    }

    pub(crate) fn archive(&self) {
        self.store
            .state
            .lock()
            .unwrap()
            .archive(&self.connection_id);
    }

    /// Hold the log lock while enqueueing, so transport IO cannot race registration.
    pub(crate) fn send_request<T>(
        &self,
        thread_id: &str,
        send: impl FnOnce() -> SentRequest<T>,
    ) -> SentRequest<T> {
        let mut store = self.store.state.lock().unwrap();
        let pending = send();
        store.update(&self.connection_id, |state, _| {
            state.threads.insert(thread_id.into());
            state.requests.insert(
                (
                    "outgoing".into(),
                    serde_json::to_string(pending.id()).unwrap(),
                ),
                RequestContext {
                    thread_id: Some(thread_id.into()),
                    method: pending.method().into(),
                    ..Default::default()
                },
            );
        });
        pending
    }

    pub(crate) fn record_line(&self, direction: &str, line: &str) {
        self.store
            .state
            .lock()
            .unwrap()
            .update(&self.connection_id, |state, sequence| {
                if state.active {
                    state.record_line(&self.connection_id, direction, line, sequence);
                }
            });
    }

    #[cfg(test)]
    pub(crate) fn messages(&self) -> Vec<Value> {
        self.store.messages(&self.connection_id).unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn thread_messages(&self, thread_id: &str) -> Vec<Value> {
        self.store.thread_messages(thread_id)
    }
}

impl LogState {
    fn record_line(&mut self, connection_id: &str, direction: &str, line: &str, sequence: u64) {
        let wire: Value = serde_json::from_str(line).unwrap_or(Value::Null);
        let context = if direction == "stderr" {
            RequestContext::default()
        } else {
            self.correlate(direction, &wire)
        };
        let scope = if context.thread_id.is_some() {
            "thread"
        } else if context.shared {
            "connection"
        } else {
            "unattributed"
        };
        if self.messages.len() >= 500 {
            self.bytes -= self.messages.pop_front().unwrap().bytes;
        }
        let value = json!({
            "connection_id": connection_id,
            "thread_id": context.thread_id,
            "session_id": context.session_id,
            "scope": scope,
            "direction": direction,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "message": line.chars().take(65536).collect::<String>(),
        });
        let bytes = serde_json::to_vec(&value).unwrap().len();
        self.bytes += bytes;
        self.messages.push_back(LogMessage {
            value,
            bytes,
            sequence,
        });
    }
    fn bind_session(&mut self, session_id: &str, thread_id: &str) {
        self.threads.insert(thread_id.into());
        self.sessions.insert(session_id.into(), thread_id.into());
        // Some adapters emit replay notifications before the new/load response.
        for entry in &mut self.messages {
            if entry.value["scope"] == "unattributed" && entry.value["session_id"] == session_id {
                entry.value["thread_id"] = thread_id.into();
                entry.value["scope"] = "thread".into();
                entry.bytes = serde_json::to_vec(&entry.value).unwrap().len();
            }
        }
        self.bytes = self.messages.iter().map(|entry| entry.bytes).sum();
        for request in self
            .requests
            .values_mut()
            .chain(self.elicitations.values_mut())
        {
            if request.thread_id.is_none() && request.session_id.as_deref() == Some(session_id) {
                request.thread_id = Some(thread_id.into());
            }
        }
    }

    fn correlate(&mut self, direction: &str, wire: &Value) -> RequestContext {
        let opposite = if direction == "incoming" {
            "outgoing"
        } else {
            "incoming"
        };
        if wire.get("method").is_none() {
            let mut context = wire
                .get("id")
                .and_then(|id| self.requests.remove(&(opposite.into(), id.to_string())))
                .unwrap_or_default();
            if context.method == "session/new"
                && let Some(session_id) = wire["result"]["sessionId"].as_str()
                && let Some(thread_id) = &context.thread_id
            {
                self.bind_session(session_id, thread_id);
                context.session_id = Some(session_id.into());
            }
            return context;
        }
        let params = &wire["params"];
        let method = wire["method"].as_str().unwrap_or_default();
        if method == "elicitation/complete" {
            return params["elicitationId"]
                .as_str()
                .and_then(|id| self.elicitations.remove(&id.to_owned()))
                .unwrap_or_default();
        }
        let request_direction = if method == "$/cancel_request" {
            direction
        } else {
            opposite
        };
        let session_id = params["sessionId"]
            .as_str()
            .or_else(|| params["scope"]["sessionId"].as_str())
            .map(str::to_owned);
        let key = wire.get("id").map(|id| (direction.into(), id.to_string()));
        let mut context = key
            .as_ref()
            .and_then(|key| self.requests.get(key))
            .cloned()
            .or_else(|| {
                // Elicitation can refer to an initiating request before a session exists.
                params
                    .get("requestId")
                    .and_then(|id| {
                        self.requests
                            .get(&(request_direction.into(), id.to_string()))
                    })
                    .cloned()
            })
            .unwrap_or_default();
        if let Some(session_id) = session_id {
            context.thread_id = context
                .thread_id
                .or_else(|| self.sessions.get(&session_id).cloned());
            context.session_id = Some(session_id);
        }
        context.shared |= context.thread_id.is_none()
            && context.session_id.is_none()
            && matches!(method, "initialize" | "authenticate" | "session/list");
        context.method = method.into();
        if method == "elicitation/create"
            && params["mode"] == "url"
            && let Some(id) = params["elicitationId"].as_str()
        {
            self.elicitations.insert(id.into(), context.clone());
        }
        if let Some(key) = key {
            self.requests.insert(key, context.clone());
        }
        context
    }
}
