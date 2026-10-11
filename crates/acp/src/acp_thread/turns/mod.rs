//! Per-prompt outcomes and conservative final-message selection.
use crate::facade::{SessionSnapshot, SessionTurn, ThreadEntry};

pub(crate) fn start(thread: &mut SessionSnapshot, entry_start: usize) {
    thread.turns.push(SessionTurn {
        user_entry_id: thread.entries[entry_start].id.clone(),
        status: "in_progress".into(),
        stop_reason: None,
        error: None,
    });
}

pub(crate) fn finish(thread: &mut SessionSnapshot) {
    if thread.status == "working" {
        return;
    }
    let Some(turn) = thread
        .turns
        .last_mut()
        .filter(|turn| turn.status == "in_progress")
    else {
        return;
    };
    turn.status = match thread.stop_reason.as_deref() {
        Some("end_turn") => "completed",
        Some("refusal") => "failed",
        Some(_) => "interrupted",
        None if thread.status == "disconnected" => "interrupted",
        None if thread.error.is_some() => "failed",
        None => "interrupted",
    }
    .into();
    turn.stop_reason = thread.stop_reason.clone();
    turn.error = thread.error.clone();
}

// Replayed entries get new local IDs. Carry outcomes over only for a verified
// identical transcript prefix; never guess by repeated prompt text alone.
pub(crate) fn restore(previous: &SessionSnapshot, replay: &mut SessionSnapshot) {
    let matched = previous
        .entries
        .iter()
        .zip(&replay.entries)
        .take_while(|(before, after)| before.kind == after.kind && before.content == after.content)
        .count();
    for turn in &previous.turns {
        let Some(start) = previous
            .entries
            .iter()
            .position(|entry| entry.id == turn.user_entry_id)
        else {
            continue;
        };
        let end = previous
            .entries
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, entry)| {
                previous
                    .turns
                    .iter()
                    .any(|next| next.user_entry_id == entry.id)
            })
            .map_or(previous.entries.len(), |(index, _)| index);
        if start < matched && end <= matched {
            let mut turn = turn.clone();
            turn.user_entry_id = replay.entries[start].id.clone();
            replay.turns.push(turn);
        }
    }
}

pub(crate) fn final_message(entries: &[ThreadEntry]) -> Vec<&ThreadEntry> {
    let relevant: Vec<_> = entries
        .iter()
        .filter(|entry| match entry.kind.as_str() {
            "tool" | "user" | "summary" | "compaction_update" => true,
            "assistant" | "thought" => {
                entry.content["type"] != "text"
                    || entry.content["text"]
                        .as_str()
                        .is_some_and(|text| !text.trim().is_empty())
            }
            _ => false,
        })
        .collect();
    let Some(last) = relevant.last().filter(|entry| entry.kind == "assistant") else {
        return Vec::new();
    };
    let mut message_id = last.message_id.as_deref();
    let mut start = relevant.len() - 1;
    while start > 0 {
        let entry = relevant[start - 1];
        if entry.kind != "assistant"
            || matches!((message_id, entry.message_id.as_deref()), (Some(a), Some(b)) if a != b)
        {
            break;
        }
        message_id = message_id.or(entry.message_id.as_deref());
        start -= 1;
    }
    relevant[start..].to_vec()
}

#[cfg(test)]
mod tests;
