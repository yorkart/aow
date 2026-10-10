//! Zed AcpThread event reduction, with the GPUI entity replaced by a serializable model.
use crate::facade::{SessionSnapshot, ThreadEntry};
use serde_json::{Value, json};

pub(crate) struct AcpThread;

impl AcpThread {
    pub(crate) fn handle_session_update(thread: &mut SessionSnapshot, update: Value) {
        let kind = update["sessionUpdate"].as_str().unwrap_or("unknown");
        match kind {
            "user_message_chunk"
            | "agent_message_chunk"
            | "agent_thought_chunk"
            | "compaction_summary_chunk" => {
                let role = match kind {
                    "user_message_chunk" => "user",
                    "agent_thought_chunk" => "thought",
                    "compaction_summary_chunk" => "summary",
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
                let content = serde_json::from_value(content.clone())
                    .ok()
                    .and_then(|block| content::from_v1(block).ok())
                    .and_then(|block| serde_json::to_value(block).ok())
                    .unwrap_or(content);
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
            "tool_call" => {
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
            "tool_call_update" => {
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
                if let Some(entry) = thread.entries.iter_mut().find(|e| e.kind == "plan") {
                    entry.content = update;
                } else {
                    Self::push(thread, "plan", update);
                }
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
            "notice" | "compaction_update" => {
                Self::push(thread, kind, update.clone());
            }
            _ => {
                Self::push(thread, "extension", update);
            }
        }
    }

    pub(crate) fn push(thread: &mut SessionSnapshot, kind: &str, content: Value) {
        thread.entries.push(ThreadEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            content,
        });
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
pub(crate) mod content;
pub(crate) mod defaults;
pub(crate) mod prompt_capabilities;
