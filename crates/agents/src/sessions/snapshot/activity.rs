use super::{content::string, model::SnapshotMessage, turn::TurnDraft};
use serde_json::{Map, Value};

pub(super) fn command_actions(record: &Map<String, Value>) -> Vec<&'static str> {
    let mut actions = Vec::new();
    if let Some(commands) = record.get("parsed_cmd").and_then(Value::as_array) {
        for command in commands {
            let action = match command.get("type").and_then(Value::as_str) {
                Some("read") => "read_files",
                Some("search") => "search_files",
                Some("list_files") => "list_files",
                Some("unknown") => "run_commands",
                _ => continue,
            };
            if !actions.contains(&action) {
                actions.push(action);
            }
        }
    }
    actions
}

pub(super) fn add_assistant_message(
    draft: &mut TurnDraft,
    message: SnapshotMessage,
    phase: Option<&str>,
    rank: u8,
) {
    if phase == Some("commentary") {
        draft.archive_provisional_final();
        draft.add_commentary(message);
    } else {
        if draft
            .final_message
            .as_ref()
            .is_some_and(|previous| previous.text != message.text)
        {
            draft.archive_provisional_final();
        }
        draft.set_final(
            message,
            if phase == Some("final_answer") {
                rank
            } else {
                1
            },
        );
    }
}

pub(super) fn tool_status(record: &Map<String, Value>, default: &'static str) -> &'static str {
    if record.get("is_error").or_else(|| record.get("isError")) == Some(&Value::Bool(true))
        || record.get("success") == Some(&Value::Bool(false))
        || record
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
        || record.get("error").is_some_and(|error| !error.is_null())
    {
        return "failed";
    }
    match string(record, "status") {
        Some("failed" | "error" | "declined") => "failed",
        Some("interrupted" | "cancelled" | "canceled") => "interrupted",
        Some("in_progress" | "running") => "in_progress",
        Some("completed" | "succeeded" | "success") => "completed",
        _ => default,
    }
}

pub(super) fn output_status(output: Option<&Value>) -> &'static str {
    match output {
        Some(Value::Object(object)) => tool_status(object, "completed"),
        Some(Value::String(text)) => {
            if let Ok(Value::Object(object)) = serde_json::from_str(text) {
                return tool_status(&object, "completed");
            }
            // Shell tools may return a text envelope instead of structured JSON.
            for line in text.lines() {
                if let Some(code) = line
                    .strip_prefix("Process exited with code ")
                    .and_then(|code| code.trim().parse::<i64>().ok())
                {
                    return if code == 0 { "completed" } else { "failed" };
                }
            }
            "completed"
        }
        _ => "completed",
    }
}

pub(super) fn abbreviated(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut summary: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        summary.push('…');
    }
    summary
}
