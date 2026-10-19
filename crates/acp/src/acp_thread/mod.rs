//! Upstream AcpThread event reduction, with the GPUI entity replaced by a serializable model.
use crate::facade::{Permission, SessionSnapshot, ThreadEntry};
use serde_json::{Value, json};
use updates::Change;

pub(crate) struct AcpThread;

impl AcpThread {
    pub(crate) fn handle_session_update(thread: &mut SessionSnapshot, mut update: Value) {
        steering::observe_activity(thread, &update);
        terminals::apply_legacy_update(thread, &update);
        terminals::remove_consumed_output(&mut update);
        let kind = update["sessionUpdate"].as_str().unwrap_or("unknown");
        match kind {
            "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
                let role = match kind {
                    "user_message_chunk" => "user",
                    "agent_thought_chunk" => "thought",
                    _ => "assistant",
                };
                let content = update["content"].clone();
                if role == "user" && steering::consume_echo(thread, &content) {
                    return;
                }
                if role == "user"
                    && content["type"] == "image"
                    && let Some(index) = thread.pending_user_images.iter().position(|image| {
                        image["data"] == content["data"] && image["mimeType"] == content["mimeType"]
                    })
                {
                    thread.pending_user_images.remove(index);
                    return;
                }
                if role == "user" && content["type"] == "text" {
                    let echoed = content["text"].as_str().unwrap_or("");
                    if !thread.pending_user_echo.is_empty()
                        && thread.pending_user_echo.starts_with(echoed)
                    {
                        thread.pending_user_echo.drain(..echoed.len());
                        return;
                    }
                }
                let content = normalize_content(content);
                let message_id = update["messageId"].as_str().map(str::to_owned);
                let index = thread.entries.len().saturating_sub(1);
                if let Some(last) = thread.entries.last_mut()
                    && last.kind == role
                    && can_merge_message_chunks(last.message_id.as_deref(), message_id.as_deref())
                    && content["type"] == "text"
                    && last.content["type"] == "text"
                {
                    let text_start = if last.message_id.is_none() && message_id.is_some() {
                        None
                    } else {
                        last.content["text"].as_str().map(str::len)
                    };
                    if last.message_id.is_none() {
                        last.message_id = message_id;
                    }
                    if let Value::String(text) = &mut last.content["text"] {
                        text.push_str(content["text"].as_str().unwrap_or(""));
                    } else {
                        last.content["text"] = content["text"].as_str().unwrap_or("").into();
                    }
                    thread.changes.push(Change::Entry { index, text_start });
                } else {
                    thread.entries.push(ThreadEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: role.into(),
                        message_id,
                        delivery: None,
                        content,
                    });
                    thread.changes.push(Change::Entry {
                        index: thread.entries.len() - 1,
                        text_start: None,
                    });
                }
            }
            "tool_call" | "tool_call_update" => {
                // A terminal can arrive before its tool acquires the reference.
                // Detail cursors which did not include it yet need its full state.
                if let Some(parts) = update["content"].as_array() {
                    for id in parts
                        .iter()
                        .filter(|part| part["type"] == "terminal")
                        .filter_map(|part| part["terminalId"].as_str())
                    {
                        if thread.terminals.contains_key(id) {
                            thread.changes.push(Change::Terminal {
                                id: id.into(),
                                output_start: None,
                            });
                        }
                    }
                }
                let id = update["toolCallId"].as_str().unwrap_or("").to_owned();
                if let Some((index, entry)) = thread
                    .entries
                    .iter_mut()
                    .enumerate()
                    .rev()
                    .find(|(_, entry)| entry.kind == "tool" && entry.id == id)
                {
                    merge(&mut entry.content, update);
                    thread.changes.push(Change::Entry {
                        index,
                        text_start: None,
                    });
                } else {
                    thread.entries.push(ThreadEntry {
                        id,
                        kind: "tool".into(),
                        message_id: None,
                        delivery: None,
                        content: update,
                    });
                    thread.changes.push(Change::Entry {
                        index: thread.entries.len() - 1,
                        text_start: None,
                    });
                }
            }
            "plan" => {
                thread.plan = update;
                thread.changes.push(Change::Field("plan"));
            }
            "current_mode_update" => {
                thread.modes["currentModeId"] = update["currentModeId"].clone();
                thread.changes.push(Change::Field("modes"));
            }
            "config_option_update" => {
                thread.config_options_supported = true;
                thread.config_options = normalize_config_options(update["configOptions"].clone())
                    .unwrap_or_else(|_| update["configOptions"].clone());
                thread.changes.push(Change::Field("config_options"));
                thread
                    .changes
                    .push(Change::Field("config_options_supported"));
            }
            "available_commands_update" => {
                thread.commands = update["availableCommands"].clone();
                thread.changes.push(Change::Field("commands"));
            }
            "usage_update" => {
                thread.usage = update;
                thread.changes.push(Change::Field("usage"));
            }
            "session_info_update" => {
                if let Some(title) = update["title"].as_str() {
                    thread.title = title.to_owned();
                    thread.changes.push(Change::Field("title"));
                }
            }
            "terminal_output_chunk" | "terminal_update" => {
                Self::push(thread, "terminal", update);
            }
            "notice" => Self::push(thread, "notice", update),
            "compaction_update" => compaction::update(thread, update),
            "compaction_summary_chunk" => compaction::append_summary(thread, update),
            _ => {
                Self::push(thread, "extension", update);
            }
        }
    }

    pub(crate) fn push(thread: &mut SessionSnapshot, kind: &str, content: Value) {
        let entries = if kind == "notice" {
            &mut thread.notices
        } else {
            &mut thread.entries
        };
        entries.push(ThreadEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            message_id: None,
            delivery: None,
            content,
        });
        thread.changes.push(if kind == "notice" {
            Change::Field("notices")
        } else {
            Change::Entry {
                index: entries.len() - 1,
                text_start: None,
            }
        });
    }

    pub(crate) fn request_permission(thread: &mut SessionSnapshot, permission: Permission) {
        if permission.kind == "permission" {
            let mut update = permission.request["toolCall"].clone();
            if update["toolCallId"].is_string() {
                update["sessionUpdate"] = "tool_call_update".into();
                Self::handle_session_update(thread, update);
            }
        }
        thread.permissions.push(permission);
    }

    pub(crate) fn new_snapshot(
        id: String,
        session_id: String,
        agent_id: String,
        cwd: String,
    ) -> SessionSnapshot {
        let now = chrono::Utc::now().to_rfc3339();
        SessionSnapshot {
            thread_id: id,
            parent: None,
            source: Default::default(),
            created_at: now.clone(),
            session_id: (!session_id.is_empty()).then_some(session_id),
            agent_id,
            cwd,
            title: "New conversation".into(),
            status: "idle".into(),
            needs_load: false,
            recovery: None,
            revision: 0,
            sync_generation: uuid::Uuid::new_v4().to_string(),
            updated_at: now.clone(),
            interacted_at: Some(now),
            entries: Vec::new(),
            turns: Vec::new(),
            plan: Value::Null,
            notices: Vec::new(),
            terminals: Default::default(),
            permissions: Vec::new(),
            modes: json!({}),
            config_options: json!([]),
            config_options_supported: false,
            auth_required: false,
            commands: json!([]),
            usage: Value::Null,
            last_prompt_usage: None,
            error: None,
            stop_reason: None,
            steering_pending: false,
            steering_untracked: false,
            steering: Default::default(),
            pending_user_echo: String::new(),
            pending_user_images: Vec::new(),
            active_prompt: None,
            changes: Default::default(),
            overview: None,
            overview_cache: Default::default(),
        }
    }
}

// Zed preserves legacy streams without IDs, but never joins two known identities.
fn can_merge_message_chunks(existing: Option<&str>, incoming: Option<&str>) -> bool {
    match (existing, incoming) {
        (Some(existing), Some(incoming)) => existing == incoming,
        _ => true,
    }
}

fn normalize_content(content: Value) -> Value {
    serde_json::from_value(content.clone())
        .ok()
        .and_then(|block| content::from_v1(block).ok())
        .and_then(|block| serde_json::to_value(block).ok())
        .unwrap_or(content)
}

fn merge(target: &mut Value, update: Value) {
    if let (Some(target), Value::Object(update)) = (target.as_object_mut(), update) {
        for (key, value) in update {
            if !value.is_null() {
                target.insert(key, value);
            }
        }
    }
}

pub(crate) mod config_options;
pub(crate) mod history;

pub(crate) fn normalize_config_options(
    value: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(config_options::from_v1(
        serde_json::from_value(value)?,
    )?)?)
}
pub(crate) mod auth_methods;
mod compaction;
pub(crate) mod content;
pub(crate) mod defaults;
pub(crate) mod overview;
pub(crate) mod prompt_capabilities;
pub(crate) mod snapshot_store;
pub(crate) mod steering;
mod terminals;
#[cfg(test)]
mod tests;
pub(crate) mod turns;
pub(crate) mod updates;
