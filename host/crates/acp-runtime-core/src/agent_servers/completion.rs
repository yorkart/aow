use crate::facade::{SessionSnapshot, TurnCompletion};
use serde_json::Value;

pub(super) fn completed_turn(
    thread: &SessionSnapshot,
    turn_id: String,
    entry_start: usize,
    usage: Option<Value>,
    previous_usage: &Value,
) -> Option<TurnCompletion> {
    let turn = thread
        .turns
        .iter()
        .find(|turn| turn.user_entry_id == thread.entries[entry_start].id)?;
    if turn.status != "completed" || turn.stop_reason.as_deref() != Some("end_turn") {
        return None;
    }
    let end = thread
        .entries
        .iter()
        .enumerate()
        .skip(entry_start + 1)
        .find(|(_, entry)| {
            thread
                .turns
                .iter()
                .any(|turn| turn.user_entry_id == entry.id)
        })
        .map(|(index, _)| index)
        .into_iter()
        .chain(
            thread
                .steering
                .pending
                .as_ref()
                .and_then(|pending| pending.new_turn_start),
        )
        .min()
        .unwrap_or(thread.entries.len());
    let conclusion = crate::acp_thread::turns::final_message(&thread.entries[entry_start..end])
        .into_iter()
        .filter(|entry| entry.kind == "assistant" && entry.content["type"] == "text")
        .filter_map(|entry| entry.content["text"].as_str())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(TurnCompletion {
        session: thread.into(),
        turn_id,
        conclusion: (!conclusion.is_empty()).then_some(conclusion),
        usage: usage.and_then(|usage| usage_delta(&usage, previous_usage, entry_start == 0)),
    })
}

// ACP response counters are cumulative. Never label context-window usage or an
// unknown restored baseline as this turn's usage, and omit counters after a reset.
fn usage_delta(current: &Value, previous: &Value, first_turn: bool) -> Option<Value> {
    let mut delta = serde_json::Map::new();
    for key in ["inputTokens", "outputTokens", "totalTokens"] {
        let before = if first_turn {
            0
        } else {
            previous[key].as_u64()?
        };
        delta.insert(
            key.into(),
            current[key].as_u64()?.checked_sub(before)?.into(),
        );
    }
    for key in ["cachedReadTokens", "cachedWriteTokens", "thoughtTokens"] {
        if let Some(after) = current[key].as_u64() {
            let before = if first_turn {
                Some(0)
            } else {
                previous[key].as_u64()
            };
            if let Some(value) = before.and_then(|before| after.checked_sub(before)) {
                delta.insert(key.into(), value.into());
            }
        }
    }
    Some(Value::Object(delta))
}
