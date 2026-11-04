use super::*;
use crate::sessions::{
    pi::Transcript,
    usage::{PiUsage, TokenUsage, TokenUsageParser},
};

pub(super) fn parse(transcript: &Transcript) -> Result<Vec<SnapshotTurn>, SnapshotError> {
    let mut turns = Vec::new();
    let mut current: Option<TurnDraft> = None;
    let mut token_events = PiUsage;
    for entry in transcript.branch()? {
        let timestamp = entry["timestamp"].as_str().map(str::to_owned);
        let message = &entry["message"];
        if entry["type"] != "message" {
            if let Some(draft) = &mut current
                && let Some(usage) = token_events.consume(entry)
            {
                TokenUsage::accumulate(&mut draft.usage, usage);
            }
            continue;
        }
        match message["role"].as_str() {
            Some("user") => {
                if let Some(draft) = &mut current
                    && draft.status == "in_progress"
                {
                    draft.status = "interrupted";
                }
                push_draft(&mut turns, current.take());
                let text = content_text(&message["content"]).unwrap_or_else(|| "[Image]".into());
                let mut draft = TurnDraft::new(entry["id"].as_str().unwrap_or_default().into());
                draft.set_user(SnapshotMessage { text, timestamp }, 1);
                current = Some(draft);
            }
            Some("assistant") => {
                let Some(draft) = &mut current else {
                    continue;
                };
                if let Some(usage) = token_events.consume(message) {
                    TokenUsage::accumulate(&mut draft.usage, usage);
                }
                draft.archive_provisional_final();
                if let Some(text) = content_text(&message["content"]) {
                    draft.set_final(
                        SnapshotMessage {
                            text,
                            timestamp: timestamp.clone(),
                        },
                        1,
                    );
                }
                for call in message["content"].as_array().into_iter().flatten() {
                    if call["type"] == "toolCall"
                        && let Some(record) = call.as_object()
                    {
                        draft.add_tool(
                            call["id"].as_str().unwrap_or_default(),
                            call["name"].as_str().unwrap_or("tool"),
                            timestamp.clone(),
                            "in_progress",
                            record,
                        );
                    }
                }
                draft.status = match message["stopReason"].as_str() {
                    Some("stop" | "length") => "completed",
                    Some("error") => "failed",
                    Some("aborted") => "interrupted",
                    _ => "in_progress",
                };
            }
            Some("toolResult") => {
                if let Some(draft) = &mut current
                    && let Some(record) = message.as_object()
                {
                    draft.finish_tool(
                        message["toolCallId"].as_str().unwrap_or_default(),
                        tool_status(record, "completed"),
                        ToolDetails::from_result(record),
                    );
                    if let Some(usage) = token_events.consume(message) {
                        TokenUsage::accumulate(&mut draft.usage, usage);
                    }
                }
            }
            _ => {}
        }
    }
    push_draft(&mut turns, current);
    Ok(turns)
}
