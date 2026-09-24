use super::*;

pub(super) fn parse_codex_like(path: &Path) -> Result<Vec<SnapshotTurn>, SnapshotError> {
    let file = File::open(path)?;
    let mut turns = Vec::new();
    let mut current: Option<TurnDraft> = None;
    let mut fallback_id = 0usize;
    // A running task can receive several user inputs, each displayed as a turn.
    // Keep those drafts together until the task's terminal event settles them.
    let mut task_start = None;

    for line in BufReader::new(file).lines() {
        let line = line?;
        if !codex_candidate_line(&line) {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(record) = record.as_object() else {
            continue;
        };
        let timestamp = string(record, "timestamp").map(str::to_owned);
        let payload = record
            .get("payload")
            .and_then(Value::as_object)
            .unwrap_or(record);
        let Some(event_type) = string(payload, "type") else {
            continue;
        };

        match (string(record, "type"), event_type) {
            (Some("event_msg"), "task_started" | "turn_started") => {
                finish_task(&mut turns, &mut current, &mut task_start);
                task_start = Some(turns.len());
                fallback_id += 1;
                current = Some(TurnDraft::new(
                    string(payload, "turn_id")
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("turn-{fallback_id}")),
                ));
            }
            (Some("event_msg"), "task_complete" | "turn_complete") => {
                let draft = ensure_draft(&mut current, &mut fallback_id);
                if let Some(text) = string(payload, "last_agent_message").and_then(clean_text) {
                    draft.set_final(SnapshotMessage { text, timestamp }, 4);
                }
                draft.status = "completed";
                finish_task(&mut turns, &mut current, &mut task_start);
            }
            (Some("event_msg"), "turn_aborted" | "task_aborted") => {
                let draft = ensure_draft(&mut current, &mut fallback_id);
                draft.status = "interrupted";
                finish_task(&mut turns, &mut current, &mut task_start);
            }
            (Some("event_msg"), "error") => {
                ensure_draft(&mut current, &mut fallback_id).status = "failed";
                if let Some(start) = task_start {
                    settle_task(&mut turns[start..], "failed");
                }
            }
            (Some("event_msg"), "user_message") => {
                if let Some(text) = payload
                    .get("message")
                    .and_then(content_text)
                    .and_then(real_user_text)
                {
                    add_codex_user(
                        &mut turns,
                        &mut current,
                        &mut fallback_id,
                        SnapshotMessage { text, timestamp },
                        2,
                        task_start.is_some(),
                    );
                }
            }
            (Some("event_msg"), "agent_message") => {
                let phase = string(payload, "phase");
                if let Some(text) = payload.get("message").and_then(content_text) {
                    add_assistant_message(
                        ensure_draft(&mut current, &mut fallback_id),
                        SnapshotMessage { text, timestamp },
                        phase,
                        3,
                    );
                }
            }
            (Some("event_msg"), "item_started" | "item_completed") => {
                let Some(item) = payload.get("item").and_then(Value::as_object) else {
                    continue;
                };
                match string(item, "type") {
                    Some("UserMessage" | "user_message") => {
                        if let Some(text) = item
                            .get("content")
                            .and_then(content_text)
                            .and_then(real_user_text)
                        {
                            add_codex_user(
                                &mut turns,
                                &mut current,
                                &mut fallback_id,
                                SnapshotMessage { text, timestamp },
                                3,
                                task_start.is_some(),
                            );
                        }
                    }
                    Some("AgentMessage" | "agent_message") if event_type == "item_completed" => {
                        if let Some(text) = item.get("content").and_then(content_text) {
                            add_assistant_message(
                                ensure_draft(&mut current, &mut fallback_id),
                                SnapshotMessage { text, timestamp },
                                string(item, "phase"),
                                3,
                            );
                        }
                    }
                    _ => {
                        let name = match string(item, "type") {
                            Some("CommandExecution" | "command_execution") => Some("exec_command"),
                            Some("FileChange" | "file_change") => Some("apply_patch"),
                            Some(
                                "McpToolCall" | "mcp_tool_call" | "DynamicToolCall"
                                | "dynamic_tool_call",
                            ) => string(item, "tool").or_else(|| string(item, "tool_name")),
                            Some("WebSearch" | "web_search") => Some("web_search"),
                            Some("Extension") if string(item, "kind") == Some("web_search") => {
                                Some("web_search")
                            }
                            _ => None,
                        };
                        if let (Some(name), Some(id)) = (name, string(item, "id")) {
                            let default = if event_type == "item_started" {
                                "in_progress"
                            } else {
                                "completed"
                            };
                            ensure_draft(&mut current, &mut fallback_id).add_tool(
                                id,
                                name,
                                timestamp,
                                tool_status(item, default),
                                item,
                            );
                        }
                    }
                }
            }
            (Some("response_item"), "function_call" | "custom_tool_call") => {
                if let (Some(id), Some(name)) =
                    (string(payload, "call_id"), string(payload, "name"))
                {
                    ensure_draft(&mut current, &mut fallback_id).add_tool(
                        id,
                        name,
                        timestamp,
                        "in_progress",
                        payload,
                    );
                }
            }
            (Some("response_item"), "function_call_output" | "custom_tool_call_output") => {
                if let (Some(draft), Some(id)) = (current.as_mut(), string(payload, "call_id")) {
                    draft.finish_tool(
                        id,
                        output_status(payload.get("output")),
                        ToolDetails::from_output(payload.get("output")),
                    );
                }
            }
            (
                Some("event_msg"),
                "exec_command_begin" | "exec_command_end" | "patch_apply_begin" | "patch_apply_end",
            ) => {
                if let Some(id) = string(payload, "call_id") {
                    let name = if event_type.starts_with("exec_command") {
                        "exec_command"
                    } else {
                        "apply_patch"
                    };
                    let default = if event_type.ends_with("begin") {
                        "in_progress"
                    } else {
                        "completed"
                    };
                    ensure_draft(&mut current, &mut fallback_id).add_tool(
                        id,
                        name,
                        timestamp,
                        tool_status(payload, default),
                        payload,
                    );
                }
            }
            (Some("response_item" | "message"), "message") => {
                let role = string(payload, "role");
                let text = payload.get("content").and_then(content_text);
                if role == Some("user") {
                    if let Some(text) = text.and_then(real_user_text) {
                        add_codex_user(
                            &mut turns,
                            &mut current,
                            &mut fallback_id,
                            SnapshotMessage { text, timestamp },
                            1,
                            task_start.is_some(),
                        );
                    }
                } else if role == Some("assistant")
                    && let Some(text) = text
                {
                    add_assistant_message(
                        ensure_draft(&mut current, &mut fallback_id),
                        SnapshotMessage { text, timestamp },
                        string(payload, "phase"),
                        2,
                    );
                }
            }
            _ => {}
        }
    }
    push_codex_draft(&mut turns, current);
    Ok(turns.into_iter().filter_map(TurnDraft::finish).collect())
}

fn push_codex_draft(turns: &mut Vec<TurnDraft>, draft: Option<TurnDraft>) {
    if let Some(draft) = draft.filter(|draft| draft.user.is_some()) {
        turns.push(draft);
    }
}

fn terminal_status(draft: &TurnDraft) -> &'static str {
    if draft.status != "in_progress" {
        draft.status
    } else if draft.final_message.is_some() {
        "completed"
    } else {
        "interrupted"
    }
}

fn settle_task(turns: &mut [TurnDraft], status: &'static str) {
    for turn in turns {
        if turn.status == "in_progress" {
            turn.status = status;
        }
    }
}

fn finish_task(
    turns: &mut Vec<TurnDraft>,
    current: &mut Option<TurnDraft>,
    task_start: &mut Option<usize>,
) {
    let status = current.as_ref().map_or("interrupted", terminal_status);
    let start = task_start.take().unwrap_or(turns.len());
    push_codex_draft(turns, current.take());
    settle_task(&mut turns[start..], status);
}

fn codex_candidate_line(line: &str) -> bool {
    // Match JSON tokens, allowing whitespace used by older transcript writers.
    ["\"event_msg\"", "\"response_item\"", "\"message\""]
        .iter()
        .any(|token| line.contains(token))
}

fn add_codex_user(
    turns: &mut Vec<TurnDraft>,
    current: &mut Option<TurnDraft>,
    fallback_id: &mut usize,
    message: SnapshotMessage,
    rank: u8,
    task_active: bool,
) {
    if current.as_ref().is_some_and(|draft| {
        draft
            .user
            .as_ref()
            .is_some_and(|user| user.text != message.text)
    }) {
        if !task_active
            && let Some(draft) = current.as_mut()
        {
            draft.status = terminal_status(draft);
        }
        push_codex_draft(turns, current.take());
    }
    ensure_draft(current, fallback_id).set_user(message, rank);
}
