use super::*;
use serde::Deserialize;
use std::{
    process::Stdio,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub(super) struct Claude;

impl AgentSessionTracker for Claude {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        let Some(pid) = context.pid else {
            return SessionResolution::NotFound;
        };
        let Some(sessions) = claude_sessions(context.environment).await else {
            return SessionResolution::Unavailable;
        };
        match claude_session_id(&sessions, pid) {
            Some(id) => SessionResolution::Resolved(SessionTarget::Id(id)),
            None => SessionResolution::NotFound,
        }
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
        match record["type"].as_str()? {
            "assistant" => {
                let message = &record["message"];
                let message_id = message["id"].as_str().map(str::to_owned);
                if message_id.is_none() || self.message_id != message_id {
                    self.reset();
                }
                self.message_id = message_id;
                let has_tool = message["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_use"));
                let terminal = matches!(
                    message["stop_reason"].as_str(),
                    None | Some("end_turn" | "max_tokens" | "stop_sequence" | "refusal")
                );
                if !terminal || has_tool {
                    self.reset();
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
            "user" => self.reset(),
            "system" if record["subtype"] == "turn_duration" => {
                // Only the native main-loop boundary emits a notification.
                let conclusion = self.conclusion.take();
                self.reset();
                return Some(TaskStopped {
                    turn_id: record["uuid"].as_str().map(str::to_owned),
                    conclusion,
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
        .filter(|id| Uuid::parse_str(id).is_ok());
    let first = ids.next()?;
    ids.all(|id| id == first).then(|| first.to_owned())
}

struct ClaudeCacheEntry {
    environment: SessionEnvironment,
    captured: Instant,
    sessions: Option<Vec<ClaudeSession>>,
}

static CLAUDE_CACHE: LazyLock<tokio::sync::Mutex<Vec<ClaudeCacheEntry>>> =
    LazyLock::new(Default::default);

async fn claude_sessions(environment: &SessionEnvironment) -> Option<Vec<ClaudeSession>> {
    // Share one CLI query across panes and clients. Cache unsupported versions
    // and failures longer; this is never run by the frequent agent badge poll.
    let mut cache = CLAUDE_CACHE.lock().await;
    cache.retain(|entry| {
        entry.captured.elapsed()
            < Duration::from_secs(if entry.sessions.is_some() { 5 } else { 30 })
    });
    if let Some(entry) = cache.iter().find(|entry| entry.environment == *environment) {
        return entry.sessions.clone();
    }
    let sessions = query_claude(environment).await;
    if cache.len() >= 16 {
        cache.remove(0);
    }
    cache.push(ClaudeCacheEntry {
        environment: environment.clone(),
        captured: Instant::now(),
        sessions: sessions.clone(),
    });
    sessions
}

async fn query_claude(environment: &SessionEnvironment) -> Option<Vec<ClaudeSession>> {
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
    tokio::time::timeout(Duration::from_secs(3), async {
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
