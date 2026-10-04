mod activity;
mod claude;
mod codex_like;
mod content;
mod hermes;
mod model;
mod tool_details;
mod turn;
use activity::{add_assistant_message, output_status, tool_status};
use claude::parse_claude;
use codex_like::parse_codex_like;
use content::{clean_text, content_has_type, content_text, real_user_text, string};
pub use model::{AgentSessionSnapshot, SnapshotError};
use model::{SnapshotMessage, SnapshotTurn};
use tool_details::ToolDetails;
use turn::{TurnDraft, ensure_draft, push_draft};

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value};

use super::{AgentSessionLocator, AgentSessionProvider};
use crate::Agent;

const MAX_SNAPSHOT_TURNS: usize = 200;
const MAX_TURN_ACTIVITIES: usize = 500;
const MAX_ACTIVITY_TEXT: usize = 1200;

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

pub(crate) fn read_hermes(
    locator: AgentSessionLocator,
) -> Result<AgentSessionSnapshot, SnapshotError> {
    validate_locator(&locator)?;
    let mut connection = super::hermes::open_db(&locator.transcript_path)?;
    let transaction = connection.transaction()?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
        [&locator.session_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(SnapshotError::NotFound);
    }
    let records = super::hermes::snapshot_records(&transaction, &locator.session_id)?;
    Ok(snapshot(locator, hermes::parse(records)))
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
    let valid_store = if locator.agent == "hermes" {
        path == root.join("state.db")
    } else {
        path.extension().and_then(|value| value.to_str()) == Some("jsonl")
    };
    if !path.starts_with(&root) || !valid_store || !path.metadata()?.is_file() {
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

#[cfg(test)]
mod tests;
