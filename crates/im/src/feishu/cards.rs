//! Display-only JSON 2.0 cards. Size the complete HTTP JSON body, including the
//! second layer of escaping around `content`, against a conservative 30 KB limit.

use std::ops::Range;

use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};

use crate::Message;

const MAX_REQUEST_BYTES: usize = 28_000;

pub(super) fn messages(message: &Message, owner: &str, delivery_id: &str) -> Result<Vec<Value>> {
    let metadata = Value::Array(message.fields.iter().map(|field| match &field.url {
        Some(url) => json!({"tag":"div","text":{"tag":"lark_md",
            "content":format!("{}：[{}]({url})", link_label(&field.label), link_label(&field.value))}}),
        None => plain_text(format!("{}：{}", field.label, field.value)),
    }).collect());
    let text = &message.body;
    let single = request(message, &metadata, text, owner, delivery_id, 1, 1)?;
    if size(&single)? <= MAX_REQUEST_BYTES {
        return Ok(vec![single]);
    }

    // There cannot be more parts than UTF-8 bytes. Reserve that many digits in
    // both the header and UUID so learning the final part count cannot overflow.
    let reserve = text.len();
    let fits = |fragment: &str| -> Result<bool> {
        Ok(size(&request(
            message,
            &metadata,
            fragment,
            owner,
            delivery_id,
            reserve,
            reserve,
        )?)? <= MAX_REQUEST_BYTES)
    };
    ensure!(fits("")?, "飞书通知基础信息过长，无法放入卡片");
    let fences = Fences::parse(text);
    let mut fragments = Vec::new();
    let mut start = 0;
    while start < text.len() {
        // Each source byte costs at least one serialized byte. Search only a
        // request-sized window, even for a conclusion containing a giant line.
        let maximum = (start + MAX_REQUEST_BYTES).min(text.len());
        let boundaries: Vec<usize> = text[start..]
            .char_indices()
            .map(|(offset, _)| start + offset)
            .take_while(|offset| *offset <= maximum)
            .chain((text.len() <= maximum).then_some(text.len()))
            .collect();
        let mut lo = 0;
        let mut hi = boundaries.len();
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if fits(&fences.fragment(text, start..boundaries[mid]))? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let mut end = boundaries[lo];
        ensure!(end > start, "飞书通知基础信息过长，无法容纳结论");
        if end < text.len() {
            // Prefer a paragraph, then a whole line, unless that wastes more
            // than half the card. Never trim: every original byte is retained.
            let fragment = &text[start..end];
            if let Some(boundary) = fragment
                .rfind("\n\n")
                .map(|index| index + 2)
                .filter(|index| *index >= fragment.len() / 2)
                .or_else(|| {
                    fragment
                        .rfind('\n')
                        .map(|index| index + 1)
                        .filter(|index| *index >= fragment.len() / 2)
                })
            {
                end = start + boundary;
            }
        }
        // Fence delimiters must remain intact. An exceptionally large delimiter
        // line cannot safely be reformatted; report this instead of losing text.
        if let Some(line) = fences
            .lines
            .iter()
            .find(|line| line.start < end && end < line.end)
        {
            end = line.start;
        }
        ensure!(end > start, "结论中的代码块标记行过长，无法分片");
        let fragment = fences.fragment(text, start..end);
        // A shorter preferred boundary can need a closing fence of its own.
        if !fits(&fragment)? {
            bail!("结论的代码块标记过长，无法放入卡片");
        }
        fragments.push(fragment);
        start = end;
    }
    let total = fragments.len();
    fragments
        .into_iter()
        .enumerate()
        .map(|(index, fragment)| {
            let message = request(
                message,
                &metadata,
                &fragment,
                owner,
                delivery_id,
                index + 1,
                total,
            )?;
            ensure!(size(&message)? <= MAX_REQUEST_BYTES, "飞书卡片超出大小限制");
            Ok(message)
        })
        .collect()
}

fn plain_text(content: String) -> Value {
    json!({"tag":"div","text":{"tag":"plain_text","content":content}})
}

fn link_label(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut result = String::new();
    for character in escaped.chars() {
        if matches!(character, '\\' | '[' | ']' | '`' | '*' | '_' | '!') {
            result.push('\\');
        }
        result.push(if matches!(character, '\n' | '\r') {
            ' '
        } else {
            character
        });
    }
    result
}

fn size(message: &Value) -> Result<usize> {
    Ok(serde_json::to_vec(message)?.len())
}

fn request(
    message: &Message,
    metadata: &Value,
    conclusion: &str,
    owner: &str,
    delivery_id: &str,
    index: usize,
    total: usize,
) -> Result<Value> {
    let title = if total == 1 {
        message.title.clone()
    } else {
        format!("{} · {index}/{total}", message.title)
    };
    let card = json!({
        "schema": "2.0",
        "config": {"update_multi":true, "width_mode":"default"},
        "header": {"title":{"tag":"plain_text","content":title},"template":if message.error {"red"} else {"blue"}},
        "body": {
            "direction":"vertical", "padding":"12px 12px 20px 12px",
            "elements":[
                {"tag":"column_set","flex_mode":"none","columns":[
                    {"tag":"column","width":"weighted","weight":1,
                     "background_style":"grey-50","padding":"12px",
                     "elements":metadata}
                ]},
                plain_text(message.body_label.clone()),
                if message.markdown { json!({"tag":"markdown","content":conclusion}) }
                else { plain_text(conclusion.to_owned()) }
            ]
        }
    });
    Ok(json!({
        "receive_id":owner,
        "msg_type":"interactive",
        "content":serde_json::to_string(&card)?,
        // Retries reuse the same ID, different fragments/turns never share it.
        "uuid":if total == 1 { delivery_id.to_owned() } else { format!("{delivery_id}-{index}") }
    }))
}

struct Fence {
    content: Range<usize>,
    opener: String,
    closer: String,
}

#[derive(Default)]
struct Fences {
    spans: Vec<Fence>,
    lines: Vec<Range<usize>>,
}

impl Fences {
    fn parse(text: &str) -> Self {
        let mut result = Self::default();
        let mut open: Option<(u8, usize, usize, String)> = None;
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let raw = line.trim_end_matches(['\r', '\n']);
            let trimmed = raw.trim_start_matches(' ');
            let indent = raw.len() - trimmed.len();
            if indent <= 3
                && let Some(marker @ (b'`' | b'~')) = trimmed.bytes().next()
            {
                let count = trimmed.bytes().take_while(|byte| *byte == marker).count();
                if let Some((old_marker, old_count, start, opener)) = &open {
                    if marker == *old_marker
                        && count >= *old_count
                        && trimmed[count..].trim().is_empty()
                    {
                        result.spans.push(Fence {
                            content: *start..offset,
                            opener: opener.clone(),
                            closer: String::from_utf8(vec![marker; *old_count]).unwrap(),
                        });
                        result.lines.push(offset..offset + line.len());
                        open = None;
                    }
                } else if count >= 3 && (marker != b'`' || !trimmed[count..].contains('`')) {
                    open = Some((marker, count, offset + line.len(), raw.to_owned()));
                    result.lines.push(offset..offset + line.len());
                }
            }
            offset += line.len();
        }
        if let Some((marker, count, start, opener)) = open {
            result.spans.push(Fence {
                content: start..text.len(),
                opener,
                closer: String::from_utf8(vec![marker; count]).unwrap(),
            });
        }
        result
    }

    fn fragment(&self, text: &str, range: Range<usize>) -> String {
        let mut result = String::new();
        if let Some(fence) = self.spans.iter().find(|fence| {
            fence.content.start <= range.start
                && range.start <= fence.content.end
                && range.start > 0
        }) {
            result.push_str(&fence.opener);
            result.push('\n');
        }
        result.push_str(&text[range.clone()]);
        if let Some(fence) = self
            .spans
            .iter()
            .find(|fence| fence.content.start <= range.end && range.end <= fence.content.end)
        {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&fence.closer);
        }
        result
    }
}

#[cfg(test)]
mod tests;
