//! Replay a finished automation's records through its existing native parser.
//! Process success supplies the completion boundary for headless agents that do
//! not persist a stop record. No agent protocol is interpreted in this reader.

use super::*;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};

pub(super) fn read_jsonl(
    mut parser: Box<dyn TaskStopParser>,
    locator: &AgentSessionLocator,
    exited_at: DateTime<Utc>,
) -> Result<Option<String>, SnapshotError> {
    let file = File::open(&locator.transcript_path)?;
    let before = file.metadata()?;
    if DateTime::<Utc>::from(before.modified()?) > exited_at {
        return Ok(None);
    }
    let mut conclusion = None;
    // After process exit, a valid final record need not have a trailing newline.
    for line in BufReader::new(file).lines() {
        let Ok(record) = serde_json::from_str::<Value>(&line?) else {
            return Ok(None);
        };
        if let Some(event) = parser.consume(&locator.session_id, &record) {
            conclusion = event.conclusion;
        }
    }
    let after = std::fs::metadata(&locator.transcript_path)?;
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || before.created().ok() != after.created().ok()
    {
        return Ok(None);
    }
    Ok(parser
        .take_conclusion()
        .or(conclusion)
        .filter(|text| !text.trim().is_empty()))
}

#[cfg(test)]
mod tests;
