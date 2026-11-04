use aow_db::hermes::{HermesRead, MessageRecord};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use super::{
    content::{decode_content, text_content},
    messages,
};

pub(in crate::sessions) fn snapshot_records(
    reader: &HermesRead<'_>,
    id: &str,
) -> anyhow::Result<Vec<Value>> {
    let preceding_user_turns = super::snapshot::MAX_SNAPSHOT_TURNS;
    if let Some(mut records) = compacted_records(reader, id)? {
        // Bound public turns after removing compaction copies, not before.
        let start = records
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, record)| record["role"] == "user")
            .nth(preceding_user_turns)
            .map_or(0, |(index, _)| index);
        records.drain(..start);
        return Ok(records);
    }
    // Keep one extra turn so the public snapshot accurately reports truncation.
    Ok(reader
        .recent_messages(id, preceding_user_turns)?
        .into_iter()
        .map(record)
        .collect())
}

pub(in crate::sessions) fn compacted_records(
    reader: &HermesRead<'_>,
    id: &str,
) -> anyhow::Result<Option<Vec<Value>>> {
    Ok(reader
        .compacted_messages(id)?
        .map(messages::reconstruct_records))
}

pub(in crate::sessions) fn record(row: MessageRecord) -> Value {
    serde_json::json!({
        "id": row.id, "session_id": row.session_id,
        "role": row.role, "content": row.content.as_deref().map(decode_content),
        "tool_call_id": row.tool_call_id,
        "tool_calls": row.tool_calls.map(|text| serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))),
        "timestamp": timestamp(row.timestamp).to_rfc3339_opts(SecondsFormat::Millis, true),
        "finish_reason": row.finish_reason,
    })
}

pub(in crate::sessions) fn timestamp(epoch: f64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis((epoch * 1000.0) as i64).unwrap_or(DateTime::UNIX_EPOCH)
}

pub(in crate::sessions) fn terminal_reply(record: &Value) -> bool {
    record["role"] == "assistant"
        && (record["tool_calls"].is_null()
            || record["tool_calls"].as_array().is_some_and(Vec::is_empty))
        && matches!(
            record["finish_reason"].as_str(),
            Some("stop" | "end_turn" | "stop_sequence")
        )
}

pub(in crate::sessions) fn public_text(record: &Value) -> Option<String> {
    let mut text = text_content(&record["content"])?;
    if record["role"] == "assistant" {
        // Hermes can store thought blocks inline as well as in separate
        // reasoning columns. Those columns are never selected above.
        for tag in ["think", "thinking", "reasoning_scratchpad"] {
            let opening = format!("<{tag}>");
            let closing = format!("</{tag}>");
            loop {
                let lower = text.to_ascii_lowercase();
                let Some(start) = lower.find(&opening) else {
                    break;
                };
                let end = lower[start + opening.len()..]
                    .find(&closing)
                    .map_or(text.len(), |end| {
                        start + opening.len() + end + closing.len()
                    });
                text.replace_range(start..end, "");
            }
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
