//! Live session association and native task-stop protocols.
//!
//! Adapters own CLI queries, caches, title formats and completion records.
//! Callers own process validation, subscription lifetime, budgets and delivery.

mod claude;
mod codex;
mod codex_like;
mod traecli;

use std::{future::Future, path::Path};

use serde::Serialize;
use serde_json::Value;

use super::{AgentSessionLocator, SessionEnvironment, SessionRoots};
use crate::Agent;

pub struct LiveSessionContext<'a> {
    /// Foreground PID, if supplied by the terminal daemon and validated by the caller.
    pub pid: Option<i32>,
    pub cwd: &'a str,
    pub title: &'a str,
    pub environment: &'a SessionEnvironment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionTarget {
    Title(String),
    Id(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionResolution {
    Resolved(SessionTarget),
    /// No active session can be associated; remove any old subscription.
    NotFound,
    /// Lookup failed temporarily; retain the existing subscription and retry.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStopped {
    pub turn_id: Option<String>,
    /// Final assistant reply captured at this boundary, never read from a later turn.
    pub conclusion: Option<String>,
}

/// Per-transcript protocol state. Created fresh on every EOF attachment.
pub trait TaskStopParser: Send {
    fn consume(&mut self, session_id: &str, record: &Value) -> Option<TaskStopped>;
    fn reset(&mut self);
}

pub trait AgentSessionTracker {
    fn resolve_live_session(
        &self,
        context: LiveSessionContext<'_>,
    ) -> impl Future<Output = SessionResolution> + Send;

    /// Native candidates, newest first. The caller applies its subscription budget.
    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator>;

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser>;
}

/// Only adapters implementing live session tracking belong in this capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingAgent {
    Claude,
    Codex,
    TraeCli,
}

impl TrackingAgent {
    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
        }
    }
}

impl Agent {
    pub fn session_tracking(self) -> Option<TrackingAgent> {
        match self {
            Self::Claude => Some(TrackingAgent::Claude),
            Self::Codex => Some(TrackingAgent::Codex),
            Self::TraeCli => Some(TrackingAgent::TraeCli),
            _ => None,
        }
    }
}

impl AgentSessionTracker for TrackingAgent {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        match self {
            Self::Claude => claude::Claude.resolve_live_session(context).await,
            Self::Codex => codex::Codex.resolve_live_session(context).await,
            Self::TraeCli => traecli::TraeCli.resolve_live_session(context).await,
        }
    }

    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator> {
        match self {
            Self::Claude => claude::Claude.candidate_sessions(target, cwd, roots),
            Self::Codex => codex::Codex.candidate_sessions(target, cwd, roots),
            Self::TraeCli => traecli::TraeCli.candidate_sessions(target, cwd, roots),
        }
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        match self {
            Self::Claude => claude::Claude.task_stop_parser(),
            Self::Codex => codex::Codex.task_stop_parser(),
            Self::TraeCli => traecli::TraeCli.task_stop_parser(),
        }
    }
}

fn text_content(value: &Value) -> Option<String> {
    let text = if let Some(text) = value.as_str() {
        text.to_owned()
    } else {
        value
            .as_array()?
            .iter()
            .filter(|part| matches!(part["type"].as_str(), Some("text" | "output_text")))
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests;
