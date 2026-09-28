use chrono::SecondsFormat;
use rusqlite::Connection;
use serde_json::Value;

use super::{
    content::{decode_content, text_content},
    database::{has_column, timestamp},
    messages,
};

// Match Hermes' get_compression_tip: explicit branches/delegates are separate
// conversations, and a live continuation takes precedence over stale siblings.
pub(super) fn compression_edges(connection: &Connection) -> String {
    let origin = if has_column(connection, "sessions", "model_config") {
        let config =
            "CASE WHEN json_valid(child.model_config) THEN child.model_config ELSE '{}' END";
        format!(
            "AND json_extract({config}, '$._branched_from') IS NULL
                 AND json_extract({config}, '$._delegate_from') IS NULL"
        )
    } else {
        String::new()
    };
    format!(
        r#"
        WITH RECURSIVE compression_edges(parent_id, child_id) AS (
            SELECT parent.id, (
                SELECT child.id FROM sessions child WHERE child.parent_session_id = parent.id
                    AND child.source IN ('cli', 'tui') {origin}
                ORDER BY CASE WHEN child.end_reason = 'compression' THEN 0
                    WHEN child.ended_at IS NULL THEN 1 ELSE 2 END,
                    COALESCE((SELECT MAX(timestamp) FROM messages WHERE session_id = child.id), child.started_at) DESC,
                    child.started_at DESC, child.id DESC LIMIT 1
            ) FROM sessions parent WHERE parent.end_reason = 'compression'
        )
    "#
    )
}

pub(in crate::sessions) fn history(connection: &Connection) -> String {
    let edges = compression_edges(connection);
    format!(
        r#"{edges}, history(id) AS (
        SELECT id FROM sessions WHERE id = ?1
        UNION
        SELECT CASE WHEN e.parent_id = h.id THEN e.child_id ELSE e.parent_id END
        FROM history h JOIN compression_edges e ON e.parent_id = h.id OR e.child_id = h.id
        WHERE e.child_id IS NOT NULL
    )
    "#
    )
}

pub(in crate::sessions) fn snapshot_records(
    connection: &Connection,
    id: &str,
) -> rusqlite::Result<Vec<Value>> {
    if messages::has_compaction(connection, id)? {
        let mut records = messages::read(connection, id)?;
        // Bound public turns after removing compaction copies, not before.
        let start = records
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, record)| record["role"] == "user")
            .nth(200)
            .map_or(0, |(index, _)| index);
        records.drain(..start);
        return Ok(records);
    }
    let active = if has_column(connection, "messages", "active") {
        "AND active = 1"
    } else {
        ""
    };
    let history = history(connection);
    // Keep one extra turn so the public snapshot accurately reports truncation.
    let sql = format!(
        r#"{history}
        SELECT id, session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason
        FROM messages WHERE session_id IN (SELECT id FROM history) {active}
        AND id >= COALESCE((SELECT id FROM messages
            WHERE session_id IN (SELECT id FROM history) AND role = 'user' {active}
            ORDER BY id DESC LIMIT 1 OFFSET 200), 0)
        ORDER BY id
    "#
    );
    connection.prepare(&sql)?.query_map([id], record)?.collect()
}

pub(in crate::sessions) fn compacted_records(
    connection: &Connection,
    id: &str,
) -> rusqlite::Result<Option<Vec<Value>>> {
    messages::has_compaction(connection, id)?
        .then(|| messages::read(connection, id))
        .transpose()
}

pub(in crate::sessions) fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let calls: Option<String> = row.get(5)?;
    let content: Option<String> = row.get(3)?;
    Ok(serde_json::json!({
        "id": row.get::<_, i64>(0)?, "session_id": row.get::<_, String>(1)?,
        "role": row.get::<_, String>(2)?, "content": content.as_deref().map(decode_content),
        "tool_call_id": row.get::<_, Option<String>>(4)?,
        "tool_calls": calls.map(|text| serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))),
        "timestamp": timestamp(row.get(6)?).to_rfc3339_opts(SecondsFormat::Millis, true),
        "finish_reason": row.get::<_, Option<String>>(7)?,
    }))
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
