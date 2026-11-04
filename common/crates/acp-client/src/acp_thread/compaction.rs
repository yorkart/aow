//! Retained compaction state follows the ACP schema's upsert/append lifecycle.
use super::updates::Change;
use super::{AcpThread, normalize_content};
use crate::facade::SessionSnapshot;
use serde_json::{Value, json};

pub(super) fn update(thread: &mut SessionSnapshot, update: Value) {
    let Some(id) = update["compactionId"].as_str() else {
        return;
    };
    let index = ensure_entry(thread, id);
    let state = &mut thread.entries[index].content;
    state["status"] = update["status"].clone();
    // Missing patches retain state; explicit null clears it.
    if let Some(summary) = update.get("summary") {
        let mut blocks = Vec::new();
        if let Some(summary) = summary.as_array() {
            for block in summary {
                append_block(&mut blocks, normalize_content(block.clone()));
            }
        }
        state["summary"] = blocks.into();
    }
    if let Some(error) = update.get("error") {
        state["error"] = error.clone();
    }
    if let Some(meta) = update.get("_meta") {
        state["_meta"] = meta.clone();
    }
    thread.changes.push(Change::Entry {
        index,
        text_start: None,
    });
}

pub(super) fn append_summary(thread: &mut SessionSnapshot, chunk: Value) {
    let Some(id) = chunk["compactionId"].as_str() else {
        return;
    };
    // A first-seen chunk starts a compaction; later chunks append without changing its status.
    let index = ensure_entry(thread, id);
    let entry = &mut thread.entries[index];
    if let Some(summary) = entry.content["summary"].as_array_mut() {
        append_block(summary, normalize_content(chunk["content"].clone()));
    }
    thread.changes.push(Change::Entry {
        index,
        text_start: None,
    });
}

fn ensure_entry(thread: &mut SessionSnapshot, id: &str) -> usize {
    thread
        .entries
        .iter()
        .position(|entry| entry.kind == "compaction_update" && entry.content["compactionId"] == id)
        .unwrap_or_else(|| {
            AcpThread::push(
                thread,
                "compaction_update",
                json!({"compactionId":id,"status":"in_progress","summary":[]}),
            );
            thread.entries.len() - 1
        })
}

fn append_block(blocks: &mut Vec<Value>, block: Value) {
    if let Some(last) = blocks.last_mut()
        && last["type"] == "text"
        && block["type"] == "text"
    {
        if let Value::String(text) = &mut last["text"] {
            text.push_str(block["text"].as_str().unwrap_or(""));
        } else {
            last["text"] = block["text"].as_str().unwrap_or("").into();
        }
    } else {
        blocks.push(block);
    }
}
