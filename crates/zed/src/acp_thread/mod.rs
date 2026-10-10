//! Zed AcpThread event reduction, with the GPUI entity replaced by a serializable model.
use crate::facade::{Permission, SessionSnapshot, ThreadEntry};
use serde_json::{Value, json};

pub(crate) struct AcpThread;

impl AcpThread {
    pub(crate) fn handle_session_update(thread: &mut SessionSnapshot, update: Value) {
        terminals::apply_legacy_update(thread, &update);
        let kind = update["sessionUpdate"].as_str().unwrap_or("unknown");
        match kind {
            "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
                let role = match kind {
                    "user_message_chunk" => "user",
                    "agent_thought_chunk" => "thought",
                    _ => "assistant",
                };
                let content = update["content"].clone();
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
                if let Some(last) = thread.entries.last_mut()
                    && last.kind == role
                    && content["type"] == "text"
                    && last.content["type"] == "text"
                {
                    let text = format!(
                        "{}{}",
                        last.content["text"].as_str().unwrap_or(""),
                        content["text"].as_str().unwrap_or("")
                    );
                    last.content["text"] = text.into();
                } else {
                    Self::push(thread, role, content);
                }
            }
            "tool_call" | "tool_call_update" => {
                let id = update["toolCallId"].as_str().unwrap_or("").to_owned();
                if let Some(entry) = thread
                    .entries
                    .iter_mut()
                    .find(|entry| entry.kind == "tool" && entry.id == id)
                {
                    merge(&mut entry.content, update);
                } else {
                    thread.entries.push(ThreadEntry {
                        id,
                        kind: "tool".into(),
                        content: update,
                    });
                }
            }
            "plan" => {
                thread.plan = update;
            }
            "current_mode_update" => {
                thread.modes["currentModeId"] = update["currentModeId"].clone();
            }
            "config_option_update" => {
                thread.config_options_supported = true;
                thread.config_options = normalize_config_options(update["configOptions"].clone())
                    .unwrap_or_else(|_| update["configOptions"].clone());
            }
            "available_commands_update" => {
                thread.commands = update["availableCommands"].clone();
            }
            "usage_update" => {
                thread.usage = update;
            }
            "session_info_update" => {
                if let Some(title) = update["title"].as_str() {
                    thread.title = title.to_owned();
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
            content,
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

    /// Upgrade snapshots written before activity and compactions had separate state.
    pub(crate) fn restore_history(thread: &mut SessionSnapshot) {
        let mut activity_boundary = false;
        for entry in std::mem::take(&mut thread.entries) {
            match entry.kind.as_str() {
                "plan" => {
                    thread.plan = entry.content;
                    activity_boundary = true;
                }
                "notice" => {
                    thread.notices.push(entry);
                    activity_boundary = true;
                }
                "compaction_update" => {
                    activity_boundary = false;
                    let length = thread.entries.len();
                    compaction::update(thread, entry.content);
                    if let Some(new) = thread.entries.get_mut(length) {
                        new.id = entry.id;
                    }
                }
                _ => {
                    if let Some(last) = thread.entries.last_mut()
                        && activity_boundary
                        && ["user", "assistant", "thought"].contains(&entry.kind.as_str())
                        && last.kind == entry.kind
                        && last.content["type"] == "text"
                        && entry.content["type"] == "text"
                    {
                        let text = format!(
                            "{}{}",
                            last.content["text"].as_str().unwrap_or(""),
                            entry.content["text"].as_str().unwrap_or("")
                        );
                        last.content["text"] = text.into();
                    } else {
                        thread.entries.push(entry);
                    }
                    activity_boundary = false;
                }
            }
        }
    }

    pub(crate) fn new_snapshot(
        id: String,
        remote_id: String,
        agent_id: String,
        cwd: String,
    ) -> SessionSnapshot {
        SessionSnapshot {
            id,
            remote_id,
            agent_id,
            cwd,
            title: "New conversation".into(),
            status: "idle".into(),
            revision: 0,
            updated_at: chrono::Utc::now().to_rfc3339(),
            entries: Vec::new(),
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
            error: None,
            stop_reason: None,
            pending_user_echo: String::new(),
            active_prompt: None,
        }
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
    if let (Some(target), Some(update)) = (target.as_object_mut(), update.as_object()) {
        for (key, value) in update {
            if !value.is_null() {
                target.insert(key.clone(), value.clone());
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
pub(crate) mod prompt_capabilities;
mod terminals;
#[cfg(test)]
mod tests;
