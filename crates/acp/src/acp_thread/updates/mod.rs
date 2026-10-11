//! Bounded change cursors, retaining positions and byte offsets rather than
//! copies of streamed content. Gaps and model replacement fall back to a snapshot.
use crate::facade::{EntryPatch, SessionSnapshot, SessionUpdates, TerminalPatch};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const HISTORY_LIMIT: usize = 2048;

#[derive(Debug)]
pub(crate) enum Change {
    Entry {
        index: usize,
        text_start: Option<usize>,
    },
    Terminal {
        id: String,
        output_start: Option<usize>,
    },
    Field(&'static str),
}

#[derive(Debug, Default)]
pub(crate) struct ChangeLog {
    pub(super) base_revision: u64,
    pub(super) history: VecDeque<(u64, Vec<Change>)>,
    pending: Vec<Change>,
}

// Detached snapshots never need the live cursor journal (or its allocation).
impl Clone for ChangeLog {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl ChangeLog {
    pub(crate) fn push(&mut self, change: Change) {
        self.pending.push(change);
    }

    pub(crate) fn commit(&mut self, revision: u64) {
        self.history
            .push_back((revision, std::mem::take(&mut self.pending)));
        while self.history.len() > HISTORY_LIMIT {
            self.base_revision = self.history.pop_front().unwrap().0;
        }
    }

    pub(crate) fn reset(&mut self, revision: u64) {
        self.base_revision = revision;
        self.history.clear();
        self.pending.clear();
    }
}

pub(crate) fn since(thread: &SessionSnapshot, generation: &str, revision: u64) -> SessionUpdates {
    since_inner(thread, generation, revision, None)
}

pub(super) fn since_selection(
    thread: &SessionSnapshot,
    generation: &str,
    revision: u64,
    range: &std::ops::Range<usize>,
    terminals: &BTreeSet<String>,
) -> SessionUpdates {
    since_inner(thread, generation, revision, Some((range, terminals)))
}

fn since_inner(
    thread: &SessionSnapshot,
    generation: &str,
    revision: u64,
    selection: Option<(&std::ops::Range<usize>, &BTreeSet<String>)>,
) -> SessionUpdates {
    if generation != thread.sync_generation
        || revision < thread.changes.base_revision
        || revision > thread.revision
    {
        return SessionUpdates::Snapshot {
            snapshot: Box::new(match selection {
                Some((range, terminals)) => {
                    super::overview::detail_snapshot(thread, range, terminals)
                }
                None => thread.clone(),
            }),
        };
    }
    if revision == thread.revision {
        return SessionUpdates::Unchanged;
    }
    let mut entries = BTreeMap::new();
    let mut terminals = BTreeMap::new();
    let mut fields = BTreeSet::new();
    for (_, changes) in thread
        .changes
        .history
        .iter()
        .filter(|(at, _)| *at > revision)
    {
        for change in changes {
            match change {
                Change::Entry { index, text_start } => {
                    if selection.is_none_or(|(range, _)| range.contains(index)) {
                        merge_offset(&mut entries, *index, *text_start);
                    }
                }
                Change::Terminal { id, output_start } => {
                    if selection.is_none_or(|(_, terminals)| terminals.contains(id)) {
                        merge_offset(&mut terminals, id.clone(), *output_start);
                    }
                }
                Change::Field(field) => {
                    fields.insert(*field);
                }
            }
        }
    }
    let entries = entries
        .into_iter()
        .map(|(index, start)| {
            let entry = &thread.entries[index];
            let index = index - selection.map_or(0, |(range, _)| range.start);
            if let Some(start) = start
                && let Some(text) = entry.content["text"]
                    .as_str()
                    .and_then(|text| text.get(start..))
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
        .collect();
    let terminals = terminals
        .into_iter()
        .map(|(id, start)| {
            let terminal = &thread.terminals[&id];
            if let Some(start) = start
                && let Some(text) = terminal["output"]
                    .as_str()
                    .and_then(|text| text.get(start..))
            {
                TerminalPatch::AppendOutput {
                    id,
                    text: text.into(),
                }
            } else {
                TerminalPatch::Replace {
                    id,
                    terminal: terminal.clone(),
                }
            }
        })
        .collect();
    let fields = fields
        .into_iter()
        .map(|field| {
            let value = match field {
                "title" => Value::from(thread.title.clone()),
                "plan" => thread.plan.clone(),
                "modes" => thread.modes.clone(),
                "config_options" => thread.config_options.clone(),
                "config_options_supported" => Value::from(thread.config_options_supported),
                "commands" => thread.commands.clone(),
                "usage" => thread.usage.clone(),
                "notices" => serde_json::to_value(&thread.notices).expect("serializable notices"),
                _ => unreachable!("unknown session field"),
            };
            (field.into(), value)
        })
        .collect();
    SessionUpdates::Delta {
        thread_id: thread.thread_id.clone(),
        sync_generation: thread.sync_generation.clone(),
        from_revision: revision,
        revision: thread.revision,
        updated_at: thread.updated_at.clone(),
        entries,
        terminals,
        fields,
    }
}

fn merge_offset<K: Ord>(map: &mut BTreeMap<K, Option<usize>>, key: K, offset: Option<usize>) {
    map.entry(key)
        .and_modify(|previous| {
            // A replacement anywhere in the interval invalidates an append cursor.
            *previous = previous.zip(offset).map(|(a, b)| a.min(b));
        })
        .or_insert(offset);
}

#[cfg(test)]
mod tests;
