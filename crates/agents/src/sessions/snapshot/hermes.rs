use super::*;
use crate::sessions::hermes::{public_text, terminal_reply};

pub(super) fn parse(records: Vec<Value>) -> Vec<SnapshotTurn> {
    let mut turns = Vec::new();
    let mut current: Option<TurnDraft> = None;
    for record in records {
        let timestamp = record["timestamp"].as_str().map(str::to_owned);
        match record["role"].as_str() {
            Some("user") => {
                let Some(text) = public_text(&record) else {
                    continue;
                };
                if let Some(draft) = current.as_mut()
                    && draft.status == "in_progress"
                {
                    draft.status = "interrupted";
                }
                push_draft(&mut turns, current.take());
                let mut draft = TurnDraft::new(format!("hermes-{}", record["id"]));
                draft.set_user(SnapshotMessage { text, timestamp }, 1);
                current = Some(draft);
            }
            Some("assistant") => {
                let Some(draft) = current.as_mut() else {
                    continue;
                };
                let terminal = terminal_reply(&record);
                if let Some(text) = public_text(&record) {
                    add_assistant_message(
                        draft,
                        SnapshotMessage {
                            text,
                            timestamp: timestamp.clone(),
                        },
                        Some(if terminal {
                            "final_answer"
                        } else {
                            "commentary"
                        }),
                        1,
                    );
                }
                if let Some(calls) = record["tool_calls"].as_array() {
                    for (index, call) in calls.iter().enumerate() {
                        let function = call.get("function").unwrap_or(call);
                        let (Some(name), Some(object)) =
                            (function["name"].as_str(), function.as_object())
                        else {
                            continue;
                        };
                        let fallback = format!("{}-{index}", record["id"]);
                        let id = call["id"].as_str().unwrap_or(&fallback);
                        draft.add_tool(id, name, timestamp.clone(), "in_progress", object);
                    }
                }
                if terminal {
                    draft.status = "completed";
                } else if matches!(
                    record["finish_reason"].as_str(),
                    Some("length" | "max_tokens" | "incomplete")
                ) {
                    draft.status = "interrupted";
                }
            }
            Some("tool") => {
                if let (Some(draft), Some(id)) = (current.as_mut(), record["tool_call_id"].as_str())
                {
                    let output = record["content"].clone();
                    let output = output
                        .as_str()
                        .and_then(|text| serde_json::from_str::<Value>(text).ok())
                        .unwrap_or(output);
                    draft.finish_tool(
                        id,
                        output_status(Some(&output)),
                        ToolDetails::from_output(Some(&output)),
                    );
                }
            }
            _ => {}
        }
    }
    push_draft(&mut turns, current);
    turns
}
