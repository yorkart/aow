//! Reconcile submission-order steering with consumption-order history. Input
//! order and the non-steering transcript must agree before reclassification.
use crate::facade::{SessionSnapshot, ThreadEntry};
use std::collections::{BTreeMap, VecDeque};

fn optional(entry: &ThreadEntry) -> bool {
    entry.kind == "steering"
        && entry.delivery.as_ref().is_some_and(|delivery| {
            matches!(
                delivery.status.as_str(),
                "failed" | "cancelled" | "unknown" | "sending"
            )
        })
}

// Native replay may combine adjacent text blocks. Split only at the saved
// ordered input boundaries; verification below still checks the whole history.
fn input_prefix(expected: &ThreadEntry, actual: &ThreadEntry) -> bool {
    if expected.content == actual.content {
        return true;
    }
    if expected.content["type"] != "text" || actual.content["type"] != "text" {
        return false;
    }
    let Some(text) = expected.content["text"]
        .as_str()
        .filter(|text| !text.is_empty())
    else {
        return false;
    };
    if !actual.content["text"]
        .as_str()
        .is_some_and(|value| value.starts_with(text))
    {
        return false;
    }
    let mut content = actual.content.clone();
    content["text"] = text.into();
    content == expected.content
}

struct Message {
    entry: ThreadEntry,
    indices: Vec<usize>,
}

fn normalize(
    entries: &[ThreadEntry],
    excluded: impl Fn(usize, &ThreadEntry) -> bool,
) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if excluded(index, entry) {
            continue;
        }
        if let Some(previous) = result.last_mut()
            && matches!(entry.kind.as_str(), "assistant" | "thought")
            && entry.kind == previous.entry.kind
            && entry.message_id.as_ref().is_some_and(|id| !id.is_empty())
            && entry.message_id == previous.entry.message_id
            && entry.content["type"] == "text"
            && previous.entry.content["type"] == "text"
            && let Some(text) = entry.content["text"].as_str()
            && let Some(before) = previous.entry.content["text"].as_str()
        {
            previous.entry.content["text"] = format!("{before}{text}").into();
            previous.indices.push(index);
        } else {
            result.push(Message {
                entry: entry.clone(),
                indices: vec![index],
            });
        }
    }
    // Concurrent calls can be replayed in a different order. Only reorder a
    // contiguous tool batch for comparison, never across a message boundary.
    let mut start = 0;
    while start < result.len() {
        if result[start].entry.kind != "tool" {
            start += 1;
            continue;
        }
        let end = result[start..]
            .iter()
            .position(|message| message.entry.kind != "tool")
            .map_or(result.len(), |offset| start + offset);
        result[start..end].sort_by(|a, b| a.entry.id.cmp(&b.entry.id));
        start = end;
    }
    result
}

fn same_message(before: &ThreadEntry, after: &ThreadEntry) -> bool {
    if before.kind != after.kind {
        return false;
    }
    if before.kind == "tool" {
        // A native replay can replace inline output with terminal references,
        // omit rawOutput, and rebuild terminal metadata. Those are display
        // representations, not new calls. Verify the stable call identity and
        // invocation/status fields while retaining the replayed tool verbatim.
        return !before.id.is_empty()
            && before.id == after.id
            && ["name", "kind", "title", "rawInput", "status"]
                .iter()
                .all(|key| before.content[key] == after.content[key]);
    }
    let content_matches = before.content == after.content
        || before.kind == "thought"
            && before.message_id.is_some()
            && before.content["type"] == "text"
            && after.content["type"] == "text"
            && before.content["text"]
                .as_str()
                .zip(after.content["text"].as_str())
                .is_some_and(|(a, b)| a.trim() == b.trim());
    // Live reasoning adds paragraph separators that history omits. This is
    // comparison-only: preserve the native text and still require its identity.
    content_matches
        && (!matches!(before.kind.as_str(), "assistant" | "thought")
            || before.message_id == after.message_id)
}

pub(super) fn reconcile(previous: &SessionSnapshot, replay: &mut SessionSnapshot) -> bool {
    if !previous
        .entries
        .iter()
        .any(|entry| entry.kind == "steering")
    {
        return false;
    }
    let inputs: Vec<_> = previous
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| matches!(entry.kind.as_str(), "user" | "steering"))
        .collect();
    let mut remote: VecDeque<_> = replay.entries.clone().into();
    let mut entries = Vec::new();
    let mut mapping = BTreeMap::new();
    let mut steering = BTreeMap::new();
    for &(index, entry) in &inputs {
        while remote.front().is_some_and(|entry| entry.kind != "user") {
            entries.push(remote.pop_front().unwrap());
        }
        if optional(entry) {
            // Delivery is uncertain: repeated content cannot identify which
            // submission was consumed. Leave ambiguous history untouched.
            if inputs
                .iter()
                .any(|(other, value)| *other != index && value.content == entry.content)
            {
                return false;
            }
            if remote.front().is_none_or(|next| !input_prefix(entry, next)) {
                continue;
            }
            if entry
                .delivery
                .as_ref()
                .is_some_and(|delivery| matches!(delivery.status.as_str(), "failed" | "cancelled"))
            {
                return false;
            }
        }
        let Some(next) = remote.front() else {
            return false;
        };
        if !input_prefix(entry, next) {
            return false;
        }
        let mut next = remote.pop_front().unwrap();
        if next.content != entry.content {
            let offset = entry.content["text"].as_str().unwrap().len();
            let suffix = next.content["text"].as_str().unwrap()[offset..].to_owned();
            let mut remainder = next.clone();
            remainder.id = uuid::Uuid::new_v4().to_string();
            remainder.content["text"] = suffix.into();
            remote.push_front(remainder);
            next.content = entry.content.clone();
        }
        let target = entries.len();
        entries.push(next);
        mapping.insert(index, target);
        if entry.kind == "steering" {
            steering.insert(target, index);
        }
    }
    // Later prompts from another client remain outside the verified prefix.
    while remote.front().is_some_and(|entry| entry.kind != "user") {
        entries.push(remote.pop_front().unwrap());
    }
    let replay_end = entries.len();
    entries.extend(remote);
    let before = normalize(&previous.entries, |_, entry| entry.kind == "steering");
    let after = normalize(&entries[..replay_end], |index, _| {
        steering.contains_key(&index)
    });
    if before.len() != after.len()
        || before
            .iter()
            .zip(&after)
            .any(|(a, b)| !same_message(&a.entry, &b.entry))
    {
        return false;
    }
    for (a, b) in before.iter().zip(&after) {
        for index in &a.indices {
            mapping.insert(*index, *b.indices.last().unwrap());
        }
    }
    let turns: Vec<_> = previous
        .turns
        .iter()
        .filter_map(|turn| {
            let index = previous
                .entries
                .iter()
                .position(|entry| entry.id == turn.user_entry_id)?;
            let target = *mapping.get(&index)?;
            let mut turn = turn.clone();
            turn.user_entry_id = entries[target].id.clone();
            Some(turn)
        })
        .collect();
    // All verification precedes mutation. Preserve rejected/unconfirmed local
    // cards even when the agent never included them in its native history.
    let mut insertions = BTreeMap::<usize, Vec<ThreadEntry>>::new();
    for &(index, entry) in &inputs {
        if let Some(&target) = mapping.get(&index) {
            entries[target].kind = entry.kind.clone();
            entries[target].delivery = entry.delivery.clone();
        } else {
            let boundary = mapping
                .range(..index)
                .map(|(_, target)| target + 1)
                .max()
                .unwrap_or(0);
            insertions.entry(boundary).or_default().push(entry.clone());
        }
    }
    for (boundary, cards) in insertions.into_iter().rev() {
        entries.splice(boundary..boundary, cards);
    }
    replay.entries = entries;
    replay.turns = turns;
    true
}
