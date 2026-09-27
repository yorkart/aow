//! Reconstruct the public transcript from Hermes' archived compaction contexts.
//!
//! archive_and_compact keeps original rows, then inserts protected head/tail
//! messages again with fresh IDs. Copies must retain their original identity:
//! global content deduplication would also erase genuinely repeated turns.
use super::*;

const SUMMARY_PREFIXES: [&str; 2] = [
    "[CONTEXT COMPACTION — REFERENCE ONLY]",
    "[CONTEXT SUMMARY]:",
];
const PRIOR: &str = "[PRIOR CONTEXT — for reference only; not a new message]";
const SUMMARY: &str = "[END OF PRIOR CONTEXT — COMPACTION SUMMARY BELOW]";

pub(super) fn has_compaction(connection: &Connection, id: &str) -> rusqlite::Result<bool> {
    if !has_column(connection, "messages", "compacted") {
        return Ok(false);
    }
    connection.query_row(
        &format!("{} SELECT EXISTS(SELECT 1 FROM messages WHERE session_id IN (SELECT id FROM history) AND compacted = 1)", history(connection)),
        [id], |row| row.get(0),
    )
}

struct Message {
    data: Value,
    key: String,
    epoch: f64,
    visible: bool,
    compacted: bool,
    summary: bool,
    merged: bool,
}

impl Message {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let mut data = record(row)?;
        let active: bool = row.get(8)?;
        let compacted: bool = row.get(9)?;
        let text = public_text(&data).unwrap_or_default();
        let prior = text
            .strip_prefix(PRIOR)
            .and_then(|text| text.split_once(SUMMARY));
        let merged = prior.is_some_and(|(_, summary)| is_summary(summary.trim()));
        let summary = is_summary(&text) || merged;
        if merged {
            // A merged handoff still carries one real message before the banner.
            // A retained message can have been wrapped by several compactions.
            let mut original = prior.unwrap().0.trim();
            while let Some(inner) = original.strip_prefix(PRIOR) {
                original = inner
                    .split_once(SUMMARY)
                    .map_or(inner, |(prior, _)| prior)
                    .trim();
            }
            data["content"] = Value::String(original.to_owned());
        }
        let key = if data["role"] == "system" {
            // Hermes appends a compaction note to a preserved system prompt.
            "system".to_owned()
        } else if data["role"] == "tool" && data["tool_call_id"].is_string() {
            // Protected tool results can be shortened by context pruning.
            format!("tool:{}", data["tool_call_id"])
        } else {
            serde_json::json!([
                data["role"],
                public_text(&data),
                data["tool_calls"],
                data["finish_reason"]
            ])
            .to_string()
        };
        Ok(Self {
            data,
            key,
            epoch: row.get(6)?,
            visible: active || compacted,
            compacted,
            summary,
            merged,
        })
    }

    fn copy_of(&self, original: &Self, anchor: &Self) -> bool {
        if self.key != original.key {
            return false;
        }
        if self.epoch == original.epoch {
            return true;
        }
        // Native _insert_message_rows either preserves a timestamp or assigns
        // successive values separated by 1us within its write transaction.
        // The summary anchors that batch. This prevents identical later turns
        // from being swallowed by a content-only suffix match.
        let step = (anchor.epoch + 1e-6) - anchor.epoch;
        let distance = self.data["id"].as_i64().unwrap() - anchor.data["id"].as_i64().unwrap();
        let expected = anchor.epoch + distance as f64 * step;
        step > 0.0 && (self.epoch - expected).abs() < step / 2.0
    }
}

fn is_summary(text: &str) -> bool {
    SUMMARY_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn copied_sequence<'a>(
    pairs: impl Iterator<Item = (&'a Message, &'a Message)> + Clone,
    anchor: &Message,
) -> bool {
    // User timestamps anchor turn identity. Adjacent assistant/tool rows may
    // acquire fresh timestamps in a mixed resumed/live context. Without a user
    // anchor, require timestamp evidence for each row as well as its content.
    let has_user = pairs.clone().any(|(copy, _)| copy.data["role"] == "user");
    pairs.into_iter().all(|(copy, original)| {
        copy.key == original.key
            && ((has_user && copy.data["role"] != "user") || copy.copy_of(original, anchor))
    })
}

pub(super) fn read(connection: &Connection, id: &str) -> rusqlite::Result<Vec<Value>> {
    let sql = format!(
        "{} SELECT id, session_id, role, content, tool_call_id, tool_calls,
        timestamp, finish_reason, active, compacted FROM messages
        WHERE session_id IN (SELECT id FROM history) ORDER BY id",
        history(connection)
    );
    let mut statement = connection.prepare(&sql)?;
    let mut sessions = std::collections::BTreeMap::<String, Vec<Message>>::new();
    for row in statement.query_map([id], Message::read)? {
        let message = row?;
        sessions
            .entry(message.data["session_id"].as_str().unwrap().to_owned())
            .or_default()
            .push(message);
    }
    let mut records: Vec<_> = sessions.into_values().flat_map(reconstruct).collect();
    records.sort_by_key(|record| record["id"].as_i64());
    Ok(records)
}

fn reconstruct(messages: Vec<Message>) -> Vec<Value> {
    // The last archive boundary is explicit. Earlier generations are separated
    // by Hermes' persisted handoff banners; recover their protected head too.
    let archive_end = messages
        .iter()
        .rposition(|message| message.compacted)
        .map(|i| i + 1);
    let mut starts = vec![0];
    for (index, message) in messages.iter().enumerate().filter(|(_, m)| m.summary) {
        let previous = *starts.last().unwrap();
        let max_head = (index - previous) / 2;
        let head = (1..=max_head)
            .rev()
            .find(|&length| {
                copied_sequence(
                    (0..length).map(|offset| {
                        (
                            &messages[index - length + offset],
                            &messages[previous + offset],
                        )
                    }),
                    message,
                )
            })
            .unwrap_or(0);
        let mut start = index - head;
        if let Some(end) = archive_end.filter(|&end| end > previous && end <= index) {
            start = start.max(end);
        }
        if start > previous {
            starts.push(start);
        }
    }
    if let Some(end) =
        archive_end.filter(|&end| end < messages.len() && end > *starts.last().unwrap())
    {
        starts.push(end);
    }
    starts.push(messages.len());

    let mut originals: Vec<(Value, bool)> = Vec::new();
    // Raw row index -> original record, for the previous model context only.
    let mut context: Vec<(usize, usize)> = Vec::new();
    for bounds in starts.windows(2) {
        let group: Vec<_> = (bounds[0]..bounds[1]).collect();
        let summary = group.iter().position(|&i| messages[i].summary);
        let Some(&anchor) = summary.and_then(|i| group.get(i)).or_else(|| group.first()) else {
            continue;
        };
        let mut copies = std::collections::HashMap::new();
        let tail_start = if let Some(summary) = summary {
            // A preserved head is a prefix of the previous context.
            let pairs = group[..summary].iter().zip(&context);
            if copied_sequence(
                pairs
                    .clone()
                    .map(|(&new, &(old, _))| (&messages[new], &messages[old])),
                &messages[anchor],
            ) {
                for (&index, &(_, origin)) in pairs {
                    copies.insert(index, origin);
                }
            }
            summary + usize::from(!messages[group[summary]].merged)
        } else {
            0
        };
        let tail = &group[tail_start..];
        // Only a suffix of the previous context can be a protected tail. Stop
        // matching at that boundary so a new identical user/assistant pair is
        // still a new turn, even when it immediately follows compaction.
        let overlap = (1..=context.len().min(tail.len()))
            .rev()
            .find(|&length| {
                copied_sequence(
                    context[context.len() - length..]
                        .iter()
                        .zip(tail)
                        .map(|(&(old, _), &new)| (&messages[new], &messages[old])),
                    &messages[anchor],
                )
            })
            .unwrap_or(0);
        for (&index, &(_, origin)) in tail.iter().zip(&context[context.len() - overlap..]) {
            copies.insert(index, origin);
        }
        let mut next = Vec::new();
        for index in group {
            let message = &messages[index];
            if message.summary && !message.merged {
                continue;
            }
            let origin = if let Some(&origin) = copies.get(&index) {
                // Rewinding a copied row also hides its archived original.
                if !message.visible {
                    originals[origin].1 = false;
                }
                origin
            } else {
                originals.push((message.data.clone(), message.visible));
                originals.len() - 1
            };
            next.push((index, origin));
        }
        context = next;
    }
    originals
        .into_iter()
        .filter_map(|(record, visible)| visible.then_some(record))
        .collect()
}
