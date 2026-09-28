use serde_json::{Map, Value};

pub(super) fn content_has_type(content: &Value, expected: &str) -> bool {
    content.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item.as_object().and_then(|item| string(item, "type")) == Some(expected))
    })
}

pub(super) fn content_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let item = item.as_object()?;
                matches!(
                    string(item, "type"),
                    Some("text" | "Text" | "input_text" | "output_text") | None
                )
                .then(|| item.get("text").and_then(Value::as_str))
                .flatten()
                .map(str::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    };
    clean_text(&text)
}

pub(super) fn clean_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(super) fn real_user_text(value: String) -> Option<String> {
    const INJECTED_PREFIXES: &[&str] = &[
        "<local-command-caveat>",
        "<local-command-stdout>",
        "<command-name>",
        "<system-reminder>",
        "<task-notification>",
        "<environment_context>",
        "<permissions instructions>",
        "<collaboration_mode>",
        "<multi_agent_mode>",
        "<post-compact-continuation>",
        "# AGENTS.md instructions",
    ];
    let value = value.trim();
    if value.is_empty()
        || INJECTED_PREFIXES
            .iter()
            .any(|prefix| value.starts_with(prefix))
    {
        return None;
    }
    Some(value.to_owned())
}

pub(super) fn string<'a>(record: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    record
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
