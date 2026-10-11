//! Browser projections: collapsed turns carry no process bodies or terminal output.
//! Indexes and cached overviews are live-only and never enter the offline snapshot.
use super::{AcpThread, updates::Change};
use crate::facade::{
    SessionSnapshot, SessionUpdates, ThreadEntry, TurnOverview, TurnOverviewPatch,
};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{collections::BTreeSet, ops::Range};

#[derive(Debug, Default)]
pub(crate) struct OverviewCache {
    generation: String,
    revision: u64,
    length: usize,
    visibility: Vec<bool>,
    // Cursors older than a boundary change cannot reuse entry positions or turn IDs.
    layout_revision: u64,
    ranges: Vec<Range<usize>>,
    turns: Vec<TurnOverview>,
}

impl Clone for OverviewCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

pub(crate) fn snapshot(thread: &mut SessionSnapshot) -> SessionSnapshot {
    refresh(thread);
    let mut snapshot = metadata(thread);
    snapshot.overview = Some(thread.overview_cache.turns.clone());
    snapshot
}

pub(crate) fn since(
    thread: &mut SessionSnapshot,
    generation: &str,
    revision: u64,
) -> SessionUpdates {
    refresh(thread);
    if generation != thread.sync_generation
        || revision < thread.changes.base_revision
        || revision < thread.overview_cache.layout_revision
        || revision > thread.revision
    {
        return SessionUpdates::Snapshot {
            snapshot: Box::new(snapshot(thread)),
        };
    }
    if revision == thread.revision {
        return SessionUpdates::Unchanged;
    }
    let changed_users: BTreeSet<_> = thread
        .changes
        .history
        .iter()
        .filter(|(at, _)| *at > revision)
        .flat_map(|(_, changes)| changes)
        .filter_map(|change| match change {
            Change::Entry { index, .. } if thread.entries[*index].kind == "user" => Some(*index),
            _ => None,
        })
        .collect();
    SessionUpdates::Overview {
        from_revision: revision,
        snapshot: Box::new(metadata(thread)),
        turns: thread
            .overview_cache
            .turns
            .iter()
            .zip(&thread.overview_cache.ranges)
            .filter(|(turn, _)| turn.revision > revision)
            .map(|(turn, range)| TurnOverviewPatch {
                id: turn.id.clone(),
                revision: turn.revision,
                user: (turn.user.is_empty() || changed_users.range(range.clone()).next().is_some())
                    .then(|| turn.user.clone()),
                conclusion: turn.conclusion.clone(),
                tool_count: turn.tool_count,
                progress_count: turn.progress_count,
                running: turn.running,
                waiting: turn.waiting,
                compacting: turn.compacting,
                status: turn.status.clone(),
                error: turn.error.clone(),
                permission_tool_ids: turn.permission_tool_ids.clone(),
            })
            .collect(),
    }
}

pub(crate) fn process(
    thread: &mut SessionSnapshot,
    turn_id: &str,
    generation: Option<&str>,
    revision: Option<u64>,
) -> Result<SessionUpdates> {
    refresh(thread);
    let index = thread
        .overview_cache
        .turns
        .iter()
        .position(|turn| turn.id == turn_id);
    ensure!(
        index.is_some(),
        "ACP turn not found; reload the conversation"
    );
    let index = index.unwrap();
    let full_range = &thread.overview_cache.ranges[index];
    let turn = &thread.overview_cache.turns[index];
    let entries = &thread.entries[full_range.clone()];
    let start = turn
        .user
        .last()
        .and_then(|user| entries.iter().position(|entry| entry.id == user.id))
        .map_or(full_range.start, |index| full_range.start + index + 1);
    let end = turn
        .conclusion
        .first()
        .and_then(|final_entry| entries.iter().position(|entry| entry.id == final_entry.id))
        .map_or(full_range.end, |index| full_range.start + index);
    let range = start..end;
    let terminals = terminal_ids(&thread.entries[range.clone()]);
    let generation = generation.filter(|_| {
        revision.is_some_and(|revision| revision >= thread.overview_cache.layout_revision)
    });
    Ok(super::updates::since_selection(
        thread,
        generation.unwrap_or(""),
        revision.unwrap_or(0),
        &range,
        &terminals,
    ))
}

// Do not clone the canonical body just to clear it afterwards.
pub(super) fn metadata(thread: &SessionSnapshot) -> SessionSnapshot {
    let mut result = AcpThread::new_snapshot(
        thread.thread_id.clone(),
        thread.session_id.clone().unwrap_or_default(),
        thread.agent_id.clone(),
        thread.cwd.clone(),
    );
    result.title = thread.title.clone();
    result.status = thread.status.clone();
    result.needs_load = thread.needs_load;
    result.revision = thread.revision;
    result.sync_generation = thread.sync_generation.clone();
    result.updated_at = thread.updated_at.clone();
    result.plan = thread.plan.clone();
    result.notices = thread.notices.clone();
    result.permissions = thread.permissions.clone();
    result.modes = thread.modes.clone();
    result.config_options = thread.config_options.clone();
    result.config_options_supported = thread.config_options_supported;
    result.auth_required = thread.auth_required;
    result.commands = thread.commands.clone();
    result.usage = thread.usage.clone();
    result.error = thread.error.clone();
    result.stop_reason = thread.stop_reason.clone();
    result
}

pub(super) fn detail_snapshot(
    thread: &SessionSnapshot,
    range: &Range<usize>,
    terminals: &BTreeSet<String>,
) -> SessionSnapshot {
    let mut result = metadata(thread);
    result.entries = thread.entries[range.clone()].to_vec();
    if range.end != thread.entries.len() {
        result.permissions.clear();
    }
    result.terminals = terminals
        .iter()
        .filter_map(|id| {
            thread
                .terminals
                .get(id)
                .map(|value| (id.clone(), value.clone()))
        })
        .collect();
    result
}

fn refresh(thread: &mut SessionSnapshot) {
    let mut cache = std::mem::take(&mut thread.overview_cache);
    if cache.generation == thread.sync_generation && cache.revision == thread.revision {
        thread.overview_cache = cache;
        return;
    }
    let changed: BTreeSet<_> = thread
        .changes
        .history
        .iter()
        .filter(|(revision, _)| *revision > cache.revision)
        .flat_map(|(_, changes)| changes)
        .filter_map(|change| match change {
            Change::Entry { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    // Visibility also defines boundaries in imported history. A cancelled tool
    // gaining output can split previously adjacent user blocks into two turns.
    let visibility_changed = changed.iter().any(|index| {
        cache
            .visibility
            .get(*index)
            .is_some_and(|previous| *previous != visible(&thread.entries[*index], thread))
    });
    let rebuild = cache.generation != thread.sync_generation
        || cache.revision < thread.changes.base_revision
        || cache.ranges.is_empty()
        || thread.entries.len() < cache.length
        || visibility_changed
        || changed
            .iter()
            .any(|index| thread.entries[*index].kind == "user");
    if rebuild {
        let ranges = ranges(thread);
        let turns: Vec<_> = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| summarize(thread, range, index + 1 == ranges.len(), None))
            .collect();
        if ranges
            .iter()
            .map(|range| range.start)
            .ne(cache.ranges.iter().map(|range| range.start))
            || turns.iter().zip(&cache.turns).any(|(next, previous)| {
                next.user
                    .iter()
                    .map(|entry| &entry.id)
                    .ne(previous.user.iter().map(|entry| &entry.id))
            })
        {
            cache.layout_revision = thread.revision;
        }
        cache.visibility = thread
            .entries
            .iter()
            .map(|entry| visible(entry, thread))
            .collect();
        cache.ranges = ranges;
        cache.turns = turns;
    } else {
        cache.visibility.extend(
            thread.entries[cache.length..]
                .iter()
                .map(|entry| visible(entry, thread)),
        );
        if let Some(last) = cache.ranges.last_mut() {
            last.end = thread.entries.len();
        }
        for (index, range) in cache.ranges.iter().enumerate() {
            if changed.range(range.clone()).next().is_some()
                || index + 1 == cache.ranges.len() && cache.length != thread.entries.len()
            {
                let user = std::mem::take(&mut cache.turns[index].user);
                cache.turns[index] =
                    summarize(thread, range, index + 1 == cache.ranges.len(), Some(user));
            }
        }
    }
    cache.generation = thread.sync_generation.clone();
    cache.revision = thread.revision;
    cache.length = thread.entries.len();
    thread.overview_cache = cache;
}

fn ranges(thread: &SessionSnapshot) -> Vec<Range<usize>> {
    let starts: BTreeSet<_> = thread
        .turns
        .iter()
        .map(|turn| turn.user_entry_id.as_str())
        .collect();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut previous_user = false;
    for (index, entry) in thread.entries.iter().enumerate() {
        if !visible(entry, thread) {
            continue;
        }
        let user = entry.kind == "user";
        if ranges.is_empty() || user && (!previous_user || starts.contains(entry.id.as_str())) {
            if let Some(last) = ranges.last_mut() {
                last.end = index;
            }
            ranges.push(index..thread.entries.len());
        }
        previous_user = user;
    }
    ranges
}

fn summarize(
    thread: &SessionSnapshot,
    range: &Range<usize>,
    latest: bool,
    user: Option<Vec<ThreadEntry>>,
) -> TurnOverview {
    let entries = &thread.entries[range.clone()];
    let id = if entries[0].kind == "user" {
        entries[0].id.clone()
    } else {
        "leading".into()
    };
    let recorded = thread.turns.iter().find(|turn| turn.user_entry_id == id);
    let waiting = latest && !thread.permissions.is_empty();
    let running = latest && thread.status == "working" || waiting;
    let mut status = recorded.map(|turn| turn.status.clone());
    if running {
        status = Some("in_progress".into());
    } else if status.as_deref() == Some("in_progress") {
        status = Some("interrupted".into());
    } else if status.is_none() && latest {
        status = if thread.error.is_some() {
            Some("failed".into())
        } else {
            thread.stop_reason.as_deref().map(|reason| {
                match reason {
                    "end_turn" => "completed",
                    "refusal" => "failed",
                    _ => "interrupted",
                }
                .into()
            })
        };
    }
    let conclusion: Vec<_> =
        if !running && status.as_deref().is_none_or(|status| status == "completed") {
            final_entries(entries)
                .into_iter()
                .filter(|entry| visible_content(&entry.content))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
    let final_ids: BTreeSet<_> = conclusion.iter().map(|entry| entry.id.as_str()).collect();
    let mut tool_count = 0;
    let mut progress_count = 0;
    let mut last_kind = "";
    for entry in entries.iter().filter(|entry| {
        entry.kind != "user" && !final_ids.contains(entry.id.as_str()) && visible(entry, thread)
    }) {
        let kind = if entry.kind == "thought" {
            "assistant"
        } else {
            entry.kind.as_str()
        };
        if kind == "tool" {
            tool_count += 1;
        } else if kind != "assistant" || last_kind != kind {
            progress_count += 1;
        }
        last_kind = kind;
    }
    TurnOverview {
        id,
        revision: thread.revision,
        user: user.unwrap_or_else(|| {
            entries
                .iter()
                .take_while(|entry| entry.kind == "user" || !visible(entry, thread))
                .filter(|entry| entry.kind == "user" && visible_content(&entry.content))
                .cloned()
                .collect()
        }),
        conclusion,
        tool_count,
        progress_count,
        running,
        waiting,
        status,
        error: recorded.and_then(|turn| turn.error.clone()),
        compacting: running
            && entries.iter().any(|entry| {
                entry.kind == "compaction_update" && entry.content["status"] == "in_progress"
            }),
        permission_tool_ids: entries
            .iter()
            .filter(|entry| {
                latest
                    && entry.kind == "tool"
                    && thread
                        .permissions
                        .iter()
                        .any(|permission| permission.request["toolCall"]["toolCallId"] == entry.id)
            })
            .map(|entry| entry.id.clone())
            .collect(),
    }
}

// Match the browser's conservative final-message boundary, including invisible
// tool endings but excluding unsupported/empty message content.
fn final_entries(entries: &[ThreadEntry]) -> Vec<&ThreadEntry> {
    let relevant: Vec<_> = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind.as_str(),
                "user" | "tool" | "summary" | "compaction_update"
            ) || matches!(entry.kind.as_str(), "assistant" | "thought")
                && visible_content(&entry.content)
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

fn visible_content(content: &Value) -> bool {
    match content["type"].as_str() {
        Some("text") => content["text"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()),
        Some("image") => content["data"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        Some("resource") => content["resource"]["uri"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        Some("resource_link") => content["uri"].as_str().is_some_and(|text| !text.is_empty()),
        _ => false,
    }
}

fn visible(entry: &ThreadEntry, thread: &SessionSnapshot) -> bool {
    match entry.kind.as_str() {
        "user" | "assistant" | "thought" => visible_content(&entry.content),
        "summary" | "compaction_update" => true,
        "tool" => {
            !matches!(
                entry.content["status"].as_str(),
                Some("cancelled" | "canceled")
            ) || thread
                .permissions
                .iter()
                .any(|permission| permission.request["toolCall"]["toolCallId"] == entry.id)
                || entry.content["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        matches!(part["type"].as_str(), Some("diff" | "terminal"))
                            || part["type"] == "content" && visible_content(&part["content"])
                    })
                })
                || entry.content["content"]
                    .as_array()
                    .is_none_or(|parts| parts.is_empty())
                    && !entry.content["rawOutput"].is_null()
        }
        _ => false,
    }
}

fn terminal_ids(entries: &[ThreadEntry]) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|entry| entry.kind == "tool")
        .flat_map(|entry| entry.content["content"].as_array().into_iter().flatten())
        .filter(|part| part["type"] == "terminal")
        .filter_map(|part| part["terminalId"].as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests;
