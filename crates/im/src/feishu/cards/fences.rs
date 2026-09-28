use std::ops::Range;

struct Fence {
    content: Range<usize>,
    opener: String,
    closer: String,
}

#[derive(Default)]
pub(super) struct Fences {
    spans: Vec<Fence>,
    pub(super) lines: Vec<Range<usize>>,
}

impl Fences {
    pub(super) fn parse(text: &str) -> Self {
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

    pub(super) fn fragment(&self, text: &str, range: Range<usize>) -> String {
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
