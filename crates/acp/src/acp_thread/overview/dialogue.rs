use super::super::updates::Change;
use crate::facade::{DialoguePatch, EntryPatch, SessionSnapshot, ThreadEntry};
use std::collections::BTreeMap;

pub(super) fn visible(entry: &ThreadEntry) -> bool {
    matches!(entry.kind.as_str(), "assistant" | "steering")
        && super::visible_content(&entry.content)
}

pub(super) fn since(
    thread: &SessionSnapshot,
    indices: Option<&[usize]>,
    revision: u64,
) -> Option<DialoguePatch> {
    let indices = indices?;
    let mut changed = BTreeMap::<usize, Option<usize>>::new();
    for (_, changes) in thread
        .changes
        .history
        .iter()
        .filter(|(at, _)| *at > revision)
    {
        for change in changes {
            if let Change::Entry { index, text_start } = change
                && let Ok(position) = indices.binary_search(index)
            {
                // A first steering card enables the dialogue projection. The
                // previous browser view may never have fetched earlier answers.
                if thread.entries[*index].kind == "steering" {
                    return Some(DialoguePatch::Reset {
                        entries: indices
                            .iter()
                            .map(|index| thread.entries[*index].clone())
                            .collect(),
                    });
                }
                changed
                    .entry(position)
                    .and_modify(|before| {
                        *before = before.zip(*text_start).map(|(a, b)| a.min(b));
                    })
                    .or_insert(*text_start);
            }
        }
    }
    if changed.is_empty() {
        return None;
    }
    Some(DialoguePatch::Delta {
        entries: changed
            .into_iter()
            .map(|(index, offset)| {
                let entry = &thread.entries[indices[index]];
                if let Some(text) =
                    offset.and_then(|offset| entry.content["text"].as_str()?.get(offset..))
                {
                    EntryPatch::AppendText {
                        index,
                        id: entry.id.clone(),
                        text: text.into(),
                    }
                } else {
                    EntryPatch::Replace {
                        index,
                        entry: entry.clone(),
                    }
                }
            })
            .collect(),
    })
}
