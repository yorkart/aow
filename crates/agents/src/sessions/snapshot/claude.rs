use super::*;

const MAX_CLAUDE_ANCESTRY: usize = 100_000;

#[derive(Debug)]
struct ClaudeNode {
    parent_uuid: Option<String>,
}

pub(super) fn parse_claude(
    path: &Path,
    expected_session_id: &str,
) -> Result<Vec<SnapshotTurn>, SnapshotError> {
    let file = File::open(path)?;
    let mut nodes = HashMap::<String, ClaudeNode>::new();
    let mut leaf_uuid = None;

    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(object) = record.as_object() else {
            continue;
        };
        if string(object, "type") == Some("last-prompt") {
            let marker_session =
                string(object, "sessionId").or_else(|| string(object, "session_id"));
            if marker_session.is_none_or(|value| value == expected_session_id) {
                leaf_uuid = string(object, "leafUuid").map(str::to_owned);
            }
        }
        let Some(uuid) = string(object, "uuid").map(str::to_owned) else {
            continue;
        };
        let parent_uuid = object
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(str::to_owned);
        nodes.insert(uuid, ClaudeNode { parent_uuid });
    }

    let selected = claude_branch(&nodes, leaf_uuid.as_deref())?;
    let file = File::open(path)?;
    let rows = BufReader::new(file).lines().filter_map(|line| {
        let line = line.ok()?;
        if !line.contains("\"user\"") && !line.contains("\"assistant\"") {
            return None;
        }
        let record = serde_json::from_str::<Value>(&line).ok()?;
        let object = record.as_object()?;
        let uuid = string(object, "uuid")?;
        selected
            .as_ref()
            .is_none_or(|branch| branch.contains(uuid))
            .then_some(record)
    });
    Ok(claude_turns(rows))
}

fn claude_branch(
    nodes: &HashMap<String, ClaudeNode>,
    leaf_uuid: Option<&str>,
) -> Result<Option<HashSet<String>>, SnapshotError> {
    let Some(mut cursor) = leaf_uuid else {
        return Ok(None);
    };
    let mut branch = HashSet::new();
    for _ in 0..MAX_CLAUDE_ANCESTRY {
        if !branch.insert(cursor.to_owned()) {
            return Err(SnapshotError::Invalid(
                "cycle in Claude parentUuid ancestry".to_owned(),
            ));
        }
        let node = nodes.get(cursor).ok_or_else(|| {
            SnapshotError::Invalid(format!("missing Claude transcript ancestor {cursor}"))
        })?;
        let Some(parent) = node.parent_uuid.as_deref() else {
            return Ok(Some(branch));
        };
        cursor = parent;
    }
    Err(SnapshotError::Invalid(
        "Claude transcript ancestry exceeds the safety limit".to_owned(),
    ))
}

pub(super) fn claude_turns(rows: impl Iterator<Item = Value>) -> Vec<SnapshotTurn> {
    let mut turns = Vec::new();
    let mut current: Option<TurnDraft> = None;
    let mut fallback_id = 0usize;

    for record in rows {
        let Some(record) = record.as_object() else {
            continue;
        };
        let timestamp = string(record, "timestamp").map(str::to_owned);
        match string(record, "type") {
            Some("user") if record.get("interruptedMessageId").is_some() => {
                if let Some(draft) = current.as_mut() {
                    draft.status = "interrupted";
                }
                push_draft(&mut turns, current.take());
            }
            Some("user") => {
                if let Some(draft) = current.as_mut()
                    && let Some(items) = record
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_array)
                {
                    for item in items.iter().filter_map(Value::as_object) {
                        if string(item, "type") == Some("tool_result")
                            && let Some(id) = string(item, "tool_use_id")
                        {
                            draft.finish_tool(
                                id,
                                tool_status(item, "completed"),
                                ToolDetails::from_result(item),
                            );
                        }
                    }
                }
                let Some(text) = claude_user_text(record) else {
                    continue;
                };
                if let Some(draft) = current.as_mut()
                    && draft.final_message.is_some()
                {
                    draft.status = "completed";
                }
                push_draft(&mut turns, current.take());
                fallback_id += 1;
                let id = string(record, "uuid")
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("turn-{fallback_id}"));
                let mut draft = TurnDraft::new(id);
                draft.set_user(SnapshotMessage { text, timestamp }, 1);
                current = Some(draft);
            }
            Some("assistant") => {
                let draft = ensure_draft(&mut current, &mut fallback_id);
                let response = claude_assistant_text(record);
                if let Some((text, terminal)) = response {
                    add_assistant_message(
                        draft,
                        SnapshotMessage {
                            text,
                            timestamp: timestamp.clone(),
                        },
                        if terminal { None } else { Some("commentary") },
                        1,
                    );
                    if terminal {
                        draft.status = if record.get("isApiErrorMessage")
                            == Some(&Value::Bool(true))
                            || record.get("error").is_some()
                        {
                            "failed"
                        } else {
                            "completed"
                        };
                    }
                }
                if let Some(items) = record
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_array)
                {
                    for item in items.iter().filter_map(Value::as_object) {
                        if string(item, "type") == Some("tool_use")
                            && let (Some(id), Some(name)) =
                                (string(item, "id"), string(item, "name"))
                        {
                            draft.add_tool(id, name, timestamp.clone(), "in_progress", item);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    push_draft(&mut turns, current);
    turns
}

fn claude_user_text(record: &Map<String, Value>) -> Option<String> {
    if ["isMeta", "isSynthetic", "isCompactSummary"]
        .iter()
        .any(|field| record.get(*field) == Some(&Value::Bool(true)))
    {
        return None;
    }
    let content = record
        .get("message")
        .and_then(Value::as_object)?
        .get("content")?;
    if content_has_type(content, "tool_result") {
        return None;
    }
    content_text(content).and_then(real_user_text)
}

fn claude_assistant_text(record: &Map<String, Value>) -> Option<(String, bool)> {
    let message = record.get("message")?.as_object()?;
    let content = message.get("content")?;
    let text = content_text(content)?;
    let stop_reason = string(message, "stop_reason");
    let terminal = matches!(
        stop_reason,
        Some("end_turn" | "max_tokens" | "stop_sequence" | "refusal")
    ) || (stop_reason.is_none() && !content_has_type(content, "tool_use"));
    Some((text, terminal))
}
