//! Live session association and native task-stop protocols.
//!
//! Adapters own CLI queries, caches, title formats and completion records.
//! Callers own process validation, subscription lifetime, budgets and delivery.

mod claude;
mod codex;
mod codex_like;
mod completed;
mod hermes;
mod traecli;

use std::{future::Future, path::Path, time::Duration};

use super::snapshot::SnapshotError;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use super::{AgentSessionLocator, SessionEnvironment, SessionRoots};
use crate::Agent;

pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(3);

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
    /// Only usage observed by this reader before the native completion boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<super::usage::TokenUsage>,
}

/// Per-transcript protocol state. Each live reader or completed run owns an instance.
pub trait TaskStopParser: Send {
    fn consume(&mut self, session_id: &str, record: &Value) -> Option<TaskStopped>;
    fn reset(&mut self);
    /// Read the pending reply only after a headless process has exited successfully.
    fn take_conclusion(&mut self) -> Option<String> {
        None
    }
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

    /// Read the result immediately after a newly created automation process exits
    /// successfully. The runner must persist it before publishing run completion.
    /// This is not a live completion detector or a resumed-session history query.
    fn completed_run_result(
        &self,
        locator: &AgentSessionLocator,
        exited_at: DateTime<Utc>,
    ) -> Result<Option<String>, SnapshotError> {
        completed::read_jsonl(self.task_stop_parser(), locator, exited_at)
    }
}

/// Only adapters implementing live session tracking belong in this capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingAgent {
    Claude,
    Codex,
    TraeCli,
    Hermes,
}

impl TrackingAgent {
    /// Resolve a session with the caller's timeout for uncached native CLI queries.
    /// Adapters without a CLI query and cached results do not consume this budget.
    pub async fn resolve_live_session_with_timeout(
        &self,
        context: LiveSessionContext<'_>,
        query_timeout: Duration,
    ) -> SessionResolution {
        match self {
            Self::Claude => {
                claude::Claude
                    .resolve_live_session_with_timeout(context, query_timeout)
                    .await
            }
            _ => self.resolve_live_session(context).await,
        }
    }

    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
            Self::Hermes => Agent::Hermes,
        }
    }
}

impl Agent {
    pub fn session_tracking(self) -> Option<TrackingAgent> {
        match self {
            Self::Claude => Some(TrackingAgent::Claude),
            Self::Codex => Some(TrackingAgent::Codex),
            Self::TraeCli => Some(TrackingAgent::TraeCli),
            Self::Hermes => Some(TrackingAgent::Hermes),
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
            Self::Hermes => hermes::Hermes.resolve_live_session(context).await,
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
            Self::Hermes => hermes::Hermes.candidate_sessions(target, cwd, roots),
        }
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        match self {
            Self::Claude => claude::Claude.task_stop_parser(),
            Self::Codex => codex::Codex.task_stop_parser(),
            Self::TraeCli => traecli::TraeCli.task_stop_parser(),
            Self::Hermes => hermes::Hermes.task_stop_parser(),
        }
    }

    fn completed_run_result(
        &self,
        locator: &AgentSessionLocator,
        exited_at: DateTime<Utc>,
    ) -> Result<Option<String>, SnapshotError> {
        if locator.agent != self.agent().id() {
            return Err(SnapshotError::Invalid(
                "session identity does not match its agent adapter".into(),
            ));
        }
        super::snapshot::validate_locator(locator)?;
        match self {
            Self::Claude => claude::Claude.completed_run_result(locator, exited_at),
            Self::Codex => codex::Codex.completed_run_result(locator, exited_at),
            Self::TraeCli => traecli::TraeCli.completed_run_result(locator, exited_at),
            Self::Hermes => hermes::Hermes.completed_run_result(locator, exited_at),
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
