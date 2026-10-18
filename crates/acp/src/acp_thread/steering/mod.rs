//! Local delivery bookkeeping, independent of agent/model identity.
mod compatibility;
#[cfg(test)]
mod tests;

use crate::facade::{InputDelivery, SessionSnapshot};
pub(crate) use compatibility::{Activity, activity};
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub(crate) struct SteeringState {
    // Status notifications can precede steering admission and its response.
    pub activity: Option<Activity>,
    pub pending: Option<PendingSteer>,
    pub detached: bool,
    pub detached_active: bool,
    pub cancelled: bool,
    pub completion: Option<crate::facade::TurnCompletion>,
    echoes: Vec<Echo>,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingSteer {
    pub id: String,
    pub entry_start: usize,
    pub content: Vec<crate::facade::Content>,
    pub prompt_required: bool,
    pub cancelled: bool,
    pub saw_idle: bool,
    pub saw_active: bool,
    pub terminal: Option<Activity>,
    pub new_turn_start: Option<usize>,
}

#[derive(Clone, Debug)]
struct Echo {
    id: String,
    text: String,
    blocks: Vec<Value>,
}

pub(crate) fn expect_echo(thread: &mut SessionSnapshot, id: String, blocks: &[Value]) {
    thread.steering.echoes.push(Echo {
        id,
        text: blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect(),
        blocks: blocks
            .iter()
            .filter(|block| block["type"] != "text")
            .cloned()
            .collect(),
    });
}

pub(crate) fn forget_echo(thread: &mut SessionSnapshot, id: &str) {
    thread.steering.echoes.retain(|echo| echo.id != id);
}

pub(crate) fn consume_echo(thread: &mut SessionSnapshot, content: &Value) -> bool {
    for echo in &mut thread.steering.echoes {
        if content["type"] == "text" {
            let text = content["text"].as_str().unwrap_or("");
            if !echo.text.is_empty() && echo.text.starts_with(text) {
                echo.text.drain(..text.len());
                return true;
            }
        } else if let Some(index) = echo.blocks.iter().position(|block| {
            block == content
                || block["type"] == "image"
                    && content["type"] == "image"
                    && block["data"] == content["data"]
                    && block["mimeType"] == content["mimeType"]
        }) {
            echo.blocks.remove(index);
            return true;
        }
    }
    false
}

pub(crate) fn delivery(
    thread: &mut SessionSnapshot,
    start: usize,
    status: &str,
    error: Option<String>,
) {
    for entry in thread.entries.iter_mut().skip(start).take_while(|entry| {
        entry
            .delivery
            .as_ref()
            .is_some_and(|delivery| delivery.status == "sending")
    }) {
        entry.delivery = Some(InputDelivery {
            status: status.into(),
            error: error.clone(),
        });
    }
}

pub(crate) fn observe_activity(thread: &mut SessionSnapshot, update: &Value) {
    let Some(activity) = activity(update) else {
        return;
    };
    thread.steering.activity = Some(activity);
    if let Some(pending) = &mut thread.steering.pending {
        match activity {
            Activity::Idle => {
                pending.saw_idle = true;
                if pending.saw_active {
                    pending.terminal = Some(activity);
                }
            }
            Activity::Active if pending.saw_idle => {
                pending.saw_active = true;
                pending.terminal = None;
                pending.new_turn_start.get_or_insert(thread.entries.len());
            }
            Activity::Failed if pending.saw_active => pending.terminal = Some(activity),
            _ => {}
        }
    }
    if thread.steering.detached {
        match activity {
            Activity::Active => {
                thread.steering.detached_active = true;
                thread.steering_untracked = false;
            }
            _ if thread.steering.detached_active => finish_detached(thread, activity),
            _ => {}
        }
    }
}

/// Reclassifying an idle fallback must not put the old turn's trailing output
/// underneath the new user's message. Full snapshot publication invalidates
/// cursors after this move; entry identities remain stable.
pub(crate) fn relocate_input(
    thread: &mut SessionSnapshot,
    start: usize,
    count: usize,
    boundary: usize,
) -> usize {
    if boundary <= start + count {
        return start;
    }
    let entries: Vec<_> = thread.entries.drain(start..start + count).collect();
    let target = boundary - count;
    thread.entries.splice(target..target, entries);
    target
}

pub(crate) fn finish_detached(thread: &mut SessionSnapshot, activity: Activity) {
    thread.steering.detached = false;
    thread.steering.detached_active = false;
    thread.steering_untracked = false;
    thread.stop_reason = thread.steering.cancelled.then(|| "cancelled".into());
    thread.error = (activity == Activity::Failed).then(|| "Agent reported a session error".into());
    // Idle is not a success signal. Preserve an unknown outcome unless the
    // adapter reported failure or the user explicitly cancelled this run.
    if let Some(turn) = thread.turns.last_mut() {
        turn.status = if thread.error.is_some() {
            "failed"
        } else if thread.steering.cancelled {
            "interrupted"
        } else {
            "unknown"
        }
        .into();
        turn.stop_reason = thread.stop_reason.clone();
        turn.error = thread.error.clone();
    }
    settle_status(thread);
}

pub(crate) fn settle_status(thread: &mut SessionSnapshot) {
    if thread.status != "disconnected" {
        thread.status = if thread.active_prompt.is_some()
            || thread.steering_pending
            || thread.steering.detached
        {
            "working"
        } else {
            "idle"
        }
        .into();
    }
    if thread.status != "working" {
        thread.steering.echoes.clear();
    }
}

pub(crate) fn disconnected(thread: &mut SessionSnapshot) {
    if let Some(pending) = thread.steering.pending.take() {
        delivery(
            thread,
            pending.entry_start,
            "unknown",
            Some("连接已断开，无法确认追加指令是否送达；请勿自动重发。".into()),
        );
    }
    thread.steering_pending = false;
    thread.steering_untracked = false;
    // Pending request identities are deliberately not persisted. Recover their
    // visible messages conservatively after a host restart as well.
    for delivery in thread
        .entries
        .iter_mut()
        .filter_map(|entry| entry.delivery.as_mut())
    {
        if delivery.status == "sending" {
            delivery.status = "unknown".into();
            delivery.error = Some("连接已断开，无法确认追加指令是否送达；请勿自动重发。".into());
        }
    }
    thread.steering = SteeringState::default();
}
