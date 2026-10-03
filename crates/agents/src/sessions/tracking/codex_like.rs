use super::*;
use crate::sessions::usage::{CodexUsage, TokenUsage};

pub(super) fn resolve(context: LiveSessionContext<'_>) -> SessionResolution {
    let title = normalized_title(context.title, context.cwd);
    if title.is_empty() {
        SessionResolution::NotFound
    } else {
        SessionResolution::Resolved(SessionTarget::Title(title))
    }
}

pub(super) fn candidates(
    agent: Agent,
    target: &SessionTarget,
    cwd: &Path,
    roots: SessionRoots,
) -> Vec<AgentSessionLocator> {
    let SessionTarget::Title(title) = target else {
        return Vec::new();
    };
    crate::sessions::list_sessions(cwd, Some(agent.id()), roots)
        .into_iter()
        .map(|session| session.locator())
        .filter(|session| title_matches(title, &session.title))
        .collect()
}

#[derive(Default)]
pub(super) struct Parser {
    turn_id: Option<String>,
    conclusion: Option<String>,
    usage: Option<TokenUsage>,
    token_events: CodexUsage,
}

impl TaskStopParser for Parser {
    fn consume(&mut self, _session_id: &str, record: &Value) -> Option<TaskStopped> {
        let payload = &record["payload"];
        match (record["type"].as_str()?, payload["type"].as_str()?) {
            ("event_msg", "task_started" | "turn_started") => {
                self.reset();
                self.turn_id = payload["turn_id"].as_str().map(str::to_owned);
            }
            ("event_msg", "task_complete" | "turn_complete") => {
                let turn_id = payload["turn_id"].as_str().map(str::to_owned);
                let same_turn =
                    self.turn_id.is_none() || turn_id.is_none() || self.turn_id == turn_id;
                let fallback = self.conclusion.take().filter(|_| same_turn);
                // The completion payload is authoritative, including an explicitly empty reply.
                let conclusion = match payload.get("last_agent_message") {
                    Some(Value::String(text)) => (!text.trim().is_empty()).then(|| text.clone()),
                    _ => fallback,
                };
                let usage = self.usage.take().filter(|_| same_turn);
                self.reset();
                return Some(TaskStopped {
                    turn_id,
                    conclusion,
                    usage,
                });
            }
            ("event_msg", "turn_aborted" | "task_aborted" | "error") => self.reset(),
            ("event_msg", "token_count") => {
                if let Some(usage) = self.token_events.consume(payload) {
                    TokenUsage::accumulate(&mut self.usage, usage);
                }
            }
            ("event_msg", "user_message") => self.conclusion = None,
            ("event_msg", "agent_message") => self.assistant(payload, "message"),
            ("event_msg", "item_completed")
                if matches!(
                    payload["item"]["type"].as_str(),
                    Some("AgentMessage" | "agent_message")
                ) =>
            {
                self.assistant(&payload["item"], "content");
            }
            ("response_item", "message") if payload["role"] == "assistant" => {
                self.assistant(payload, "content");
            }
            ("response_item", "message") if payload["role"] == "user" => self.conclusion = None,
            ("response_item", "function_call" | "custom_tool_call" | "web_search_call") => {
                self.conclusion = None;
            }
            _ => {}
        }
        None
    }

    fn reset(&mut self) {
        self.turn_id = None;
        self.conclusion = None;
        self.usage = None;
    }
}

impl Parser {
    fn assistant(&mut self, message: &Value, content: &str) {
        // Older writers omit phase. Never use an explicitly intermediate response.
        self.conclusion = match message["phase"].as_str() {
            None | Some("final_answer") => text_content(&message[content]),
            _ => None,
        };
    }
}

fn title_matches(title: &str, candidate: &str) -> bool {
    let candidate = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(prefix) = title
        .strip_suffix("...")
        .or_else(|| title.strip_suffix('…'))
    {
        prefix.chars().count() >= 8 && candidate.starts_with(prefix)
    } else {
        title == candidate
    }
}

fn normalized_title(raw: &str, cwd: &str) -> String {
    fn trim(value: &str) -> &str {
        value.trim_matches(|c: char| c.is_whitespace() || "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏●✳✶✻✽✢·".contains(c))
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let raw = trim(&collapsed);
    let raw = raw
        .strip_prefix("[ ! ] Action Required")
        .or_else(|| raw.strip_prefix("[ . ] Action Required"))
        .unwrap_or(raw)
        .trim_start_matches([' ', '|']);
    let mut parts: Vec<_> = raw
        .split(" | ")
        .map(trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.last().copied() == Path::new(cwd).file_name().and_then(|p| p.to_str()) {
        parts.pop();
    }
    parts
        .into_iter()
        .filter(|part| {
            ![
                "Ready",
                "Working",
                "Thinking",
                "Waiting",
                "Starting",
                "Action required",
                "Needs input",
                "Codex",
                "TraeCode CLI",
                "Claude Code",
            ]
            .iter()
            .any(|status| part.eq_ignore_ascii_case(status))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_matching_ignores_status_decoration_but_rejects_generic_or_short_prefixes() {
        for raw in [
            "⠋ 修复终端会话 | demo",
            "[ ! ] Action Required | 修复终端会话 ⠙ | demo",
            "[ . ] Action Required | 修复终端会话 | demo",
        ] {
            assert_eq!(normalized_title(raw, "/workspace/demo"), "修复终端会话");
        }
        for raw in ["demo", "⠋ demo", "Working | demo", "Codex | demo", ""] {
            assert_eq!(normalized_title(raw, "/workspace/demo"), "");
        }
        assert!(title_matches(
            "A long conversation...",
            "A long conversation with details"
        ));
        assert!(!title_matches("Short...", "Short title"));
        assert!(title_matches("Same title", " Same   title "));
        assert!(!title_matches("Same title", "Another title"));
    }
}
