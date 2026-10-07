use super::*;
use crate::sessions::usage::{ClaudeUsage, TokenUsage, TokenUsageParser};
use serde::Deserialize;
use std::{
    process::Stdio,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;

pub(super) struct Claude;

impl Claude {
    pub(super) async fn resolve_live_session_with_timeout(
        &self,
        context: LiveSessionContext<'_>,
        query_timeout: Duration,
    ) -> SessionResolution {
        let Some(pid) = context.pid else {
            return SessionResolution::NotFound;
        };
        let Some(sessions) = claude_sessions(context.environment, query_timeout).await else {
            return SessionResolution::Unavailable;
        };
        match claude_session_id(&sessions, pid) {
            Some(id) => SessionResolution::Resolved(SessionTarget::Id(id)),
            None => SessionResolution::NotFound,
        }
    }
}

impl AgentSessionTracker for Claude {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        self.resolve_live_session_with_timeout(context, DEFAULT_QUERY_TIMEOUT)
            .await
    }

    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        _cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator> {
        let SessionTarget::Id(id) = target else {
            return Vec::new();
        };
        crate::sessions::find_session(crate::CLAUDE.id, id, roots)
            .map(|session| vec![session.locator()])
            .unwrap_or_default()
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        Box::new(Parser::default())
    }
}

#[derive(Default)]
struct Parser {
    message_id: Option<String>,
    conclusion: Option<String>,
    usage: Option<TokenUsage>,
    token_events: ClaudeUsage,
}

impl TaskStopParser for Parser {
    fn consume(&mut self, session_id: &str, record: &Value) -> Option<TaskStopped> {
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || record
                .get("sessionId")
                .or_else(|| record.get("session_id"))
                .and_then(Value::as_str)
                .is_some_and(|id| id != session_id)
        {
            return None;
        }
        if record["interruptedMessageId"].as_str().is_some() || record["isApiErrorMessage"] == true
        {
            self.reset();
            return None;
        }
        match record["type"].as_str()? {
            "assistant" => {
                let message = &record["message"];
                let message_id = message["id"].as_str().map(str::to_owned);
                if message_id.is_none() || self.message_id != message_id {
                    self.clear_reply();
                }
                self.message_id = message_id;
                if let Some(usage) = self.token_events.consume(message) {
                    TokenUsage::accumulate(&mut self.usage, usage);
                }
                let has_tool = message["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_use"));
                let terminal = matches!(
                    message["stop_reason"].as_str(),
                    None | Some("end_turn" | "max_tokens" | "stop_sequence" | "refusal")
                );
                if !terminal || has_tool {
                    self.clear_reply();
                } else if let Some(text) = text_content(&message["content"]) {
                    // Claude can serialize separate content blocks of the same
                    // API message in successive records. Preserve all text blocks.
                    if let Some(conclusion) = &mut self.conclusion {
                        conclusion.push_str("\n\n");
                        conclusion.push_str(&text);
                    } else {
                        self.conclusion = Some(text);
                    }
                }
            }
            // Includes tool results and Stop-hook continuations: a previous answer
            // is no longer the conclusion once the conversation continues.
            "user" => self.clear_reply(),
            "system" if record["subtype"] == "turn_duration" => {
                // Only the native main-loop boundary emits a notification.
                let usage = self.usage.take();
                let conclusion = self.take_conclusion();
                return Some(TaskStopped {
                    turn_id: record["uuid"].as_str().map(str::to_owned),
                    conclusion,
                    usage,
                });
            }
            "system"
                if matches!(
                    record["subtype"].as_str(),
                    Some("compact_boundary" | "api_error")
                ) =>
            {
                self.reset()
            }
            _ => {}
        }
        None
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn take_conclusion(&mut self) -> Option<String> {
        let conclusion = self.conclusion.take();
        self.reset();
        conclusion
    }
}

impl Parser {
    fn clear_reply(&mut self) {
        self.message_id = None;
        self.conclusion = None;
    }
}

#[derive(Clone, Deserialize)]
struct ClaudeSession {
    pid: Option<i32>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

fn claude_session_id(sessions: &[ClaudeSession], pid: i32) -> Option<String> {
    let mut ids = sessions
        .iter()
        .filter(|session| session.pid == Some(pid))
        .filter_map(|session| session.session_id.as_deref())
        .filter(|id| aow_id::is_valid_id(id));
    let first = ids.next()?;
    ids.all(|id| id == first).then(|| first.to_owned())
}

struct ClaudeCacheEntry {
    environment: SessionEnvironment,
    captured: Instant,
    sessions: Option<Vec<ClaudeSession>>,
}

#[derive(Default)]
struct ClaudeSessionCache {
    entries: Vec<ClaudeCacheEntry>,
}

static CLAUDE_CACHE: LazyLock<tokio::sync::Mutex<ClaudeSessionCache>> =
    LazyLock::new(Default::default);

async fn claude_sessions(
    environment: &SessionEnvironment,
    query_timeout: Duration,
) -> Option<Vec<ClaudeSession>> {
    // Share one CLI query across panes and clients. Cache unsupported versions
    // and failures longer; this is never run by the frequent agent badge poll.
    CLAUDE_CACHE
        .lock()
        .await
        .sessions(environment, query_claude(environment, query_timeout))
        .await
}

impl ClaudeSessionCache {
    async fn sessions(
        &mut self,
        environment: &SessionEnvironment,
        query: impl std::future::Future<Output = Option<Vec<ClaudeSession>>>,
    ) -> Option<Vec<ClaudeSession>> {
        self.entries.retain(|entry| {
            entry.captured.elapsed()
                < Duration::from_secs(if entry.sessions.is_some() { 5 } else { 30 })
        });
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.environment == *environment)
        {
            return entry.sessions.clone();
        }
        let sessions = query.await;
        if self.entries.len() >= 16 {
            self.entries.remove(0);
        }
        self.entries.push(ClaudeCacheEntry {
            environment: environment.clone(),
            captured: Instant::now(),
            sessions: sessions.clone(),
        });
        sessions
    }
}

async fn query_claude(
    environment: &SessionEnvironment,
    timeout: Duration,
) -> Option<Vec<ClaudeSession>> {
    let mut command = tokio::process::Command::new("claude");
    command
        .args(["agents", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("CLAUDE_CONFIG_DIR")
        .envs(environment);
    // No shell interpolation, prompts, hooks or user config writes.
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    tokio::time::timeout(timeout, async {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests;
