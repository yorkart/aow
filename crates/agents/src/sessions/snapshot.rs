mod claude;
mod codex_like;
mod tool_details;
use claude::parse_claude;
use codex_like::parse_codex_like;
use tool_details::ToolDetails;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;

use super::{AgentSessionLocator, AgentSessionProvider};
use crate::Agent;

const MAX_SNAPSHOT_TURNS: usize = 200;
const MAX_TURN_ACTIVITIES: usize = 500;
const MAX_ACTIVITY_TEXT: usize = 1200;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("agent session is no longer available")]
    NotFound,
    #[error("invalid agent session transcript: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSessionSnapshot {
    session_id: String,
    agent: &'static str,
    title: String,
    captured_at: String,
    status: &'static str,
    turns: Vec<SnapshotTurn>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SnapshotTurn {
    id: String,
    status: &'static str,
    user: SnapshotMessage,
    #[serde(rename = "final")]
    final_message: Option<SnapshotMessage>,
    activities: Vec<SnapshotActivity>,
    activities_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SnapshotMessage {
    text: String,
    timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SnapshotActivity {
    id: String,
    kind: &'static str,
    // Public progress text or the tool name. Tool payloads live in details;
    // reasoning records are never part of the snapshot.
    text: String,
    timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    // Semantic categories from parsed commands, without command text or paths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    actions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<ToolDetails>,
}

impl SnapshotActivity {
    fn merge_details(&mut self, details: Option<ToolDetails>) {
        if let Some(details) = details {
            self.details
                .get_or_insert_with(ToolDetails::default)
                .merge(details);
        }
        if self
            .details
            .as_ref()
            .and_then(|details| details.exit_code)
            .is_some_and(|code| code != 0)
        {
            self.status = Some("failed");
        }
    }
}

#[derive(Debug)]
struct TurnDraft {
    id: String,
    user: Option<SnapshotMessage>,
    user_rank: u8,
    final_message: Option<SnapshotMessage>,
    final_rank: u8,
    status: &'static str,
    activities: Vec<SnapshotActivity>,
    next_activity: usize,
    activities_truncated: bool,
}

impl TurnDraft {
    fn new(id: String) -> Self {
        Self {
            id,
            user: None,
            user_rank: 0,
            final_message: None,
            final_rank: 0,
            status: "in_progress",
            activities: Vec::new(),
            next_activity: 0,
            activities_truncated: false,
        }
    }

    fn set_user(&mut self, message: SnapshotMessage, rank: u8) {
        if self
            .user
            .as_ref()
            .is_some_and(|value| value.text == message.text)
        {
            if rank > self.user_rank {
                self.user_rank = rank;
            }
            return;
        }
        if self.user.is_none() || rank > self.user_rank {
            self.user = Some(message);
            self.user_rank = rank;
        }
    }

    fn set_final(&mut self, message: SnapshotMessage, rank: u8) {
        if self
            .final_message
            .as_ref()
            .is_some_and(|value| value.text == message.text)
        {
            if rank > self.final_rank {
                self.final_rank = rank;
            }
            return;
        }
        if self.final_message.is_none() || rank >= self.final_rank {
            self.final_message = Some(message);
            self.final_rank = rank;
        }
    }

    fn add_commentary(&mut self, message: SnapshotMessage) {
        let text = abbreviated(&message.text, MAX_ACTIVITY_TEXT);
        if self
            .activities
            .last()
            .is_some_and(|item| item.kind == "commentary" && item.text == text)
        {
            return;
        }
        self.next_activity += 1;
        self.push_activity(SnapshotActivity {
            id: format!("progress-{}", self.next_activity),
            kind: "commentary",
            text,
            timestamp: message.timestamp,
            status: None,
            actions: Vec::new(),
            details: None,
        });
    }

    fn push_activity(&mut self, activity: SnapshotActivity) {
        if self.activities.len() == MAX_TURN_ACTIVITIES {
            self.activities.remove(0);
            self.activities_truncated = true;
        }
        self.activities.push(activity);
    }

    // Older transcripts and Claude do not always mark commentary explicitly.
    // A provisional answer followed by another message/tool becomes progress.
    fn archive_provisional_final(&mut self) {
        if self.final_rank == 1 {
            if let Some(message) = self.final_message.take() {
                self.add_commentary(message);
            }
            self.final_rank = 0;
        }
    }

    fn add_tool(
        &mut self,
        call_id: &str,
        name: &str,
        timestamp: Option<String>,
        status: &'static str,
        record: &Map<String, Value>,
    ) {
        let id = format!("tool-{call_id}");
        let actions = command_actions(record);
        let details = ToolDetails::from_call(record);
        if let Some(item) = self.activities.iter_mut().find(|item| item.id == id) {
            item.status = Some(status);
            item.merge_details(details);
            if !actions.is_empty() {
                item.actions = actions;
            }
            return;
        }
        self.archive_provisional_final();
        self.push_activity(SnapshotActivity {
            id,
            kind: "tool",
            text: abbreviated(name, 160),
            timestamp,
            status: Some(status),
            actions,
            details,
        });
        if self.status == "completed" {
            self.status = "in_progress";
        }
    }

    fn finish_tool(&mut self, call_id: &str, status: &'static str, details: Option<ToolDetails>) {
        let id = format!("tool-{call_id}");
        if let Some(item) = self.activities.iter_mut().find(|item| item.id == id) {
            item.status = Some(status);
            item.merge_details(details);
        }
    }

    fn finish(mut self) -> Option<SnapshotTurn> {
        if self.status != "in_progress" {
            for item in &mut self.activities {
                if item.status == Some("in_progress") {
                    item.status = Some(if self.status == "interrupted" {
                        "interrupted"
                    } else {
                        "unknown"
                    });
                }
            }
        }
        Some(SnapshotTurn {
            id: self.id,
            status: self.status,
            user: self.user?,
            final_message: self.final_message,
            activities: self.activities,
            activities_truncated: self.activities_truncated,
        })
    }
}

pub fn read(locator: AgentSessionLocator) -> Result<AgentSessionSnapshot, SnapshotError> {
    let provider = Agent::from_id(locator.agent)
        .and_then(Agent::sessions)
        .ok_or_else(|| SnapshotError::Invalid(format!("unsupported agent {}", locator.agent)))?;
    provider.read_snapshot(locator)
}

pub(crate) fn read_claude(
    locator: AgentSessionLocator,
) -> Result<AgentSessionSnapshot, SnapshotError> {
    validate_locator(&locator)?;
    let turns = parse_claude(&locator.transcript_path, &locator.session_id)?;
    Ok(snapshot(locator, turns))
}

pub(crate) fn read_codex_like(
    locator: AgentSessionLocator,
) -> Result<AgentSessionSnapshot, SnapshotError> {
    validate_locator(&locator)?;
    let turns = parse_codex_like(&locator.transcript_path)?;
    Ok(snapshot(locator, turns))
}

pub(super) fn validate_locator(locator: &AgentSessionLocator) -> Result<(), SnapshotError> {
    let path = locator
        .transcript_path
        .canonicalize()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => SnapshotError::NotFound,
            _ => SnapshotError::Io(error),
        })?;
    let root = locator.trusted_root.canonicalize()?;
    if !path.starts_with(&root)
        || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || !path.metadata()?.is_file()
    {
        return Err(SnapshotError::Invalid(
            "transcript escaped its trusted history root".to_owned(),
        ));
    }
    Ok(())
}

fn snapshot(locator: AgentSessionLocator, mut turns: Vec<SnapshotTurn>) -> AgentSessionSnapshot {
    let truncated = turns.len() > MAX_SNAPSHOT_TURNS;
    if truncated {
        turns.drain(..turns.len() - MAX_SNAPSHOT_TURNS);
    }
    let status = turns.last().map_or("completed", |turn| turn.status);
    AgentSessionSnapshot {
        session_id: locator.session_id,
        agent: locator.agent,
        title: locator.title,
        captured_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        status,
        turns,
        truncated,
    }
}

fn command_actions(record: &Map<String, Value>) -> Vec<&'static str> {
    let mut actions = Vec::new();
    if let Some(commands) = record.get("parsed_cmd").and_then(Value::as_array) {
        for command in commands {
            let action = match command.get("type").and_then(Value::as_str) {
                Some("read") => "read_files",
                Some("search") => "search_files",
                Some("list_files") => "list_files",
                Some("unknown") => "run_commands",
                _ => continue,
            };
            if !actions.contains(&action) {
                actions.push(action);
            }
        }
    }
    actions
}

fn add_assistant_message(
    draft: &mut TurnDraft,
    message: SnapshotMessage,
    phase: Option<&str>,
    rank: u8,
) {
    if phase == Some("commentary") {
        draft.archive_provisional_final();
        draft.add_commentary(message);
    } else {
        if draft
            .final_message
            .as_ref()
            .is_some_and(|previous| previous.text != message.text)
        {
            draft.archive_provisional_final();
        }
        draft.set_final(
            message,
            if phase == Some("final_answer") {
                rank
            } else {
                1
            },
        );
    }
}

fn tool_status(record: &Map<String, Value>, default: &'static str) -> &'static str {
    if record.get("is_error").or_else(|| record.get("isError")) == Some(&Value::Bool(true))
        || record.get("success") == Some(&Value::Bool(false))
        || record
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
        || record.get("error").is_some_and(|error| !error.is_null())
    {
        return "failed";
    }
    match string(record, "status") {
        Some("failed" | "error" | "declined") => "failed",
        Some("interrupted" | "cancelled" | "canceled") => "interrupted",
        Some("in_progress" | "running") => "in_progress",
        Some("completed" | "succeeded" | "success") => "completed",
        _ => default,
    }
}

fn output_status(output: Option<&Value>) -> &'static str {
    match output {
        Some(Value::Object(object)) => tool_status(object, "completed"),
        Some(Value::String(text)) => {
            if let Ok(Value::Object(object)) = serde_json::from_str(text) {
                return tool_status(&object, "completed");
            }
            // Shell tools may return a text envelope instead of structured JSON.
            for line in text.lines() {
                if let Some(code) = line
                    .strip_prefix("Process exited with code ")
                    .and_then(|code| code.trim().parse::<i64>().ok())
                {
                    return if code == 0 { "completed" } else { "failed" };
                }
            }
            "completed"
        }
        _ => "completed",
    }
}

fn abbreviated(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut summary: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        summary.push('…');
    }
    summary
}

fn ensure_draft<'a>(
    current: &'a mut Option<TurnDraft>,
    fallback_id: &mut usize,
) -> &'a mut TurnDraft {
    current.get_or_insert_with(|| {
        *fallback_id += 1;
        TurnDraft::new(format!("turn-{}", *fallback_id))
    })
}

fn push_draft(turns: &mut Vec<SnapshotTurn>, draft: Option<TurnDraft>) {
    if let Some(turn) = draft.and_then(TurnDraft::finish) {
        turns.push(turn);
    }
}

fn content_has_type(content: &Value, expected: &str) -> bool {
    content.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item.as_object().and_then(|item| string(item, "type")) == Some(expected))
    })
}

fn content_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let item = item.as_object()?;
                matches!(
                    string(item, "type"),
                    Some("text" | "Text" | "input_text" | "output_text") | None
                )
                .then(|| item.get("text").and_then(Value::as_str))
                .flatten()
                .map(str::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    };
    clean_text(&text)
}

fn clean_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn real_user_text(value: String) -> Option<String> {
    const INJECTED_PREFIXES: &[&str] = &[
        "<local-command-caveat>",
        "<local-command-stdout>",
        "<command-name>",
        "<system-reminder>",
        "<task-notification>",
        "<environment_context>",
        "<permissions instructions>",
        "<collaboration_mode>",
        "<multi_agent_mode>",
        "<post-compact-continuation>",
        "# AGENTS.md instructions",
    ];
    let value = value.trim();
    if value.is_empty()
        || INJECTED_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
    {
        return None;
    }
    Some(value.to_owned())
}

fn string<'a>(record: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    record
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
