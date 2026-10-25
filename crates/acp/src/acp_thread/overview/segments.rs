use crate::facade::{ProcessSegment, SessionSnapshot, ThreadEntry};
use std::collections::BTreeSet;

fn segment(id: &str) -> ProcessSegment {
    ProcessSegment {
        id: id.into(),
        tool_count: 0,
        progress_count: 0,
        permission_tool_ids: Vec::new(),
    }
}

pub(super) fn summarize(
    entries: &[ThreadEntry],
    thread: &SessionSnapshot,
    final_ids: &BTreeSet<&str>,
    id: &str,
    latest: bool,
) -> Vec<ProcessSegment> {
    let mut segments = vec![segment(id)];
    let mut last_kinds = vec![""];
    let mut previous_text: Option<(&str, usize)> = None;
    for entry in entries {
        if entry.kind == "steering" {
            if super::visible_content(&entry.content) {
                segments.push(segment(&entry.id));
                last_kinds.push("");
            }
            continue;
        }
        let mut index = segments.len() - 1;
        // Match the browser's chunk coalescing: a steer does not split an
        // identified text message, but tools/thoughts/other content do.
        if entry.kind == "assistant" && entry.content["type"] == "text" {
            if let Some(message_id) = entry.message_id.as_deref().filter(|id| !id.is_empty()) {
                if let Some((previous_id, previous_segment)) = previous_text
                    && previous_id == message_id
                {
                    index = previous_segment;
                }
                previous_text = Some((message_id, index));
            } else {
                previous_text = None;
            }
        } else {
            previous_text = None;
        }
        if entry.kind == "user"
            || final_ids.contains(entry.id.as_str())
            || !super::visible(entry, thread)
        {
            continue;
        }
        let current = &mut segments[index];
        let kind = if entry.kind == "thought" {
            "assistant"
        } else {
            entry.kind.as_str()
        };
        if kind == "tool" {
            current.tool_count += 1;
            if latest
                && thread
                    .permissions
                    .iter()
                    .any(|permission| permission.request["toolCall"]["toolCallId"] == entry.id)
            {
                current.permission_tool_ids.push(entry.id.clone());
            }
        } else if kind != "assistant" || last_kinds[index] != kind {
            current.progress_count += 1;
        }
        last_kinds[index] = kind;
    }
    segments
}
