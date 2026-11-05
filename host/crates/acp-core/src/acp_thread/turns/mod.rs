//! Per-prompt outcomes and conservative final-message selection.
use crate::facade::{SessionSnapshot, SessionTurn, ThreadEntry};
mod replay;

pub(crate) fn start(thread: &mut SessionSnapshot, entry_start: usize) {
    start_with_id(thread, entry_start, uuid::Uuid::new_v4().to_string());
}

pub(crate) fn start_with_id(thread: &mut SessionSnapshot, entry_start: usize, id: String) {
    // Plans belong to a turn; retain the result until another turn is admitted.
    thread.data.plan = serde_json::Value::Null;
    super::AcpThread::clear_session_meta(thread);
    thread.session_meta_suppressed = false;
    thread.data.turns.push(SessionTurn {
        id,
        user_entry_id: thread.data.entries[entry_start].id.clone(),
        status: "in_progress".into(),
        stop_reason: None,
        error: None,
    });
}

pub(crate) fn finish(thread: &mut SessionSnapshot) {
    if thread.data.status != "working" {
        for turn in thread
            .data
            .turns
            .iter_mut()
            .filter(|turn| turn.status == "in_progress")
        {
            finish_record(
                turn,
                &thread.data.status,
                thread.data.stop_reason.clone(),
                thread.data.error.clone(),
            );
        }
    }
    // A prior prompt can finish while a detached steering turn is still running.
    settle_receipts(thread);
}

pub(crate) fn finish_at(thread: &mut SessionSnapshot, entry_start: usize) {
    let id = &thread.data.entries[entry_start].id;
    if let Some(turn) = thread
        .data
        .turns
        .iter_mut()
        .find(|turn| &turn.user_entry_id == id)
    {
        finish_record(
            turn,
            &thread.data.status,
            thread.data.stop_reason.clone(),
            thread.data.error.clone(),
        );
    }
    settle_receipts(thread);
}

fn settle_receipts(thread: &mut SessionSnapshot) {
    // Until acknowledgement, steering input/output can still belong to either
    // turn. The handler relocates the input and registers any new turn first.
    if thread.steering.pending.is_some() {
        return;
    }
    for receipt in thread
        .data
        .prompt_receipts
        .values_mut()
        .filter(|r| r.status == "in_progress")
    {
        let Some(turn) = thread
            .data
            .turns
            .iter()
            .find(|turn| turn.user_entry_id == receipt.user_entry_id)
        else {
            if thread.data.status != "working" {
                receipt.status = "interrupted".into();
                receipt.error = Some("Prompt outcome unavailable after session recovery".into());
                receipt.finished_at = Some(chrono::Utc::now().to_rfc3339());
            }
            continue;
        };
        if turn.status == "in_progress" {
            continue;
        }
        receipt.status = turn.status.clone();
        receipt.stop_reason = turn.stop_reason.clone();
        receipt.error = turn.error.clone();
        receipt.finished_at = Some(chrono::Utc::now().to_rfc3339());
        if let Some(start) = thread
            .data
            .entries
            .iter()
            .position(|e| e.id == turn.user_entry_id)
        {
            let end = thread
                .data
                .entries
                .iter()
                .enumerate()
                .skip(start + 1)
                .find(|(_, e)| thread.data.turns.iter().any(|t| t.user_entry_id == e.id))
                .map_or(thread.data.entries.len(), |(index, _)| index);
            if thread.data.entries[start..end].iter().any(|entry| {
                entry.kind == "steering"
                    && entry.delivery.as_ref().is_some_and(|delivery| {
                        matches!(delivery.status.as_str(), "sending" | "unknown")
                    })
            }) {
                // Recovery cannot reconstruct an unacknowledged steering boundary.
                receipt.status = "interrupted".into();
                receipt.stop_reason = None;
                receipt.error.get_or_insert_with(|| {
                    "Prompt result boundary unavailable after unacknowledged steering".into()
                });
                continue;
            }
            let text = final_message(&thread.data.entries[start..end])
                .iter()
                .filter_map(|entry| entry.content["text"].as_str())
                .collect::<String>();
            receipt.conclusion = (!text.trim().is_empty()).then_some(text);
        }
    }
}

fn finish_record(
    turn: &mut SessionTurn,
    status: &str,
    stop_reason: Option<String>,
    error: Option<String>,
) {
    turn.status = match stop_reason.as_deref() {
        Some("end_turn") => "completed",
        Some("refusal") => "failed",
        Some(_) => "interrupted",
        None if status == "disconnected" => "interrupted",
        None if error.is_some() => "failed",
        None => "interrupted",
    }
    .into();
    turn.stop_reason = stop_reason;
    turn.error = error;
}

// Replayed entries get new local IDs. Carry outcomes over only for a verified
// identical transcript prefix; never guess by repeated prompt text alone.
pub(crate) fn restore(previous: &SessionSnapshot, replay: &mut SessionSnapshot) {
    replay.prompt_receipts = previous.prompt_receipts.clone();
    if replay::reconcile(previous, replay) {
        return;
    }
    let matched = previous
        .entries
        .iter()
        .zip(&replay.entries)
        .take_while(|(before, after)| {
            (before.kind == after.kind || before.kind == "steering" && after.kind == "user")
                && before.content == after.content
                && (before.message_id.is_none()
                    || after.message_id.is_none()
                    || before.message_id == after.message_id)
                && (before.kind != "steering"
                    || before
                        .delivery
                        .as_ref()
                        .is_some_and(|delivery| delivery.status == "injected"))
        })
        .count();
    for (before, after) in previous
        .entries
        .iter()
        .zip(&mut replay.entries)
        .take(matched)
    {
        after.kind = before.kind.clone();
        after.delivery = before.delivery.clone();
    }
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
            "tool" | "user" | "steering" | "summary" | "compaction_update" => true,
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
