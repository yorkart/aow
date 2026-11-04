use serde_json::Value;

pub(super) fn decode_content(content: &str) -> Value {
    // Hermes stores multimodal content with an unambiguous NUL-prefixed marker.
    // Never expose an undecodable structured payload (which can contain images).
    content.strip_prefix("\0json:").map_or_else(
        || Value::String(content.to_owned()),
        |json| serde_json::from_str(json).unwrap_or(Value::Null),
    )
}

pub(super) fn content_text(content: &str) -> Option<String> {
    text_content(&decode_content(content))
}

pub(super) fn text_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(text_content)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        Value::Object(part)
            if matches!(
                part.get("type").and_then(Value::as_str),
                Some("text" | "input_text" | "output_text")
            ) =>
        {
            part.get("text").and_then(Value::as_str).map(str::to_owned)
        }
        _ => None,
    }
}
