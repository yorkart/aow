use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

use aow_protocol::{
    FileSearchFile, FileSearchMatch, FileSearchMode, FileSearchRequest, FileSearchResult,
};
use serde::Deserialize;

pub(super) const MAX_RESULTS: usize = 2000;

#[derive(Default)]
pub(super) struct Results {
    files: BTreeMap<String, FileSearchFile>,
    pub count: usize,
    pub truncated: bool,
}

#[derive(Deserialize)]
struct Text {
    text: Option<String>,
}
#[derive(Deserialize)]
struct Range {
    start: usize,
    end: usize,
}
#[derive(Deserialize)]
struct MatchData {
    path: Text,
    lines: Text,
    line_number: u64,
    submatches: Vec<Range>,
}
#[derive(Deserialize)]
struct Message {
    r#type: String,
    data: Option<serde_json::Value>,
}

impl Results {
    pub fn ingest(&mut self, bytes: &[u8], request: &FileSearchRequest) {
        if self.count >= MAX_RESULTS {
            return;
        }
        if request.mode == FileSearchMode::Name {
            let Ok(path) = std::str::from_utf8(bytes) else {
                self.truncated = true;
                return;
            };
            let lower = path.to_lowercase();
            if request
                .query
                .split_whitespace()
                .all(|word| lower.contains(&word.to_lowercase()))
                && let Some(file) = self.file(path, &request.root)
            {
                self.files.insert(file.relative_path.clone(), file);
                self.count += 1;
            }
            return;
        }
        let Ok(message) = serde_json::from_slice::<Message>(bytes) else {
            self.truncated = true;
            return;
        };
        if message.r#type != "match" {
            return;
        }
        let Some(data) = message
            .data
            .and_then(|data| serde_json::from_value::<MatchData>(data).ok())
        else {
            self.truncated = true;
            return;
        };
        let (Some(path), Some(text)) = (data.path.text, data.lines.text) else {
            self.truncated = true;
            return;
        };
        let Some(mut file) = self.file(&path, &request.root) else {
            return;
        };
        // Ripgrep's offsets are bytes; the UI and Monaco both use UTF-16 columns.
        let text = text.strip_suffix('\n').unwrap_or(&text);
        let mut previous_end = 0;
        let mut column = 0;
        for range in data.submatches {
            if range.start < previous_end
                || range.end < range.start
                || range.end > text.len()
                || !text.is_char_boundary(range.start)
                || !text.is_char_boundary(range.end)
            {
                self.truncated = true;
                continue;
            }
            column += text[previous_end..range.start].encode_utf16().count();
            let length = text[range.start..range.end].encode_utf16().count();
            file.matches.push(preview(
                text,
                range.start,
                range.end,
                data.line_number,
                column + 1,
                length,
            ));
            previous_end = range.end;
            column += length;
            self.count += 1;
            if self.count >= MAX_RESULTS {
                break;
            }
        }
        if !file.matches.is_empty() {
            self.files
                .entry(file.relative_path.clone())
                .and_modify(|existing| existing.matches.append(&mut file.matches))
                .or_insert(file);
        }
    }

    fn file(&mut self, raw: &str, root: &str) -> Option<FileSearchFile> {
        let path = Path::new(raw);
        if path
            .components()
            .any(|part| !matches!(part, Component::CurDir | Component::Normal(_)))
        {
            self.truncated = true;
            return None;
        }
        let relative: std::path::PathBuf = path
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .collect();
        Some(FileSearchFile {
            path: Path::new(root)
                .join(&relative)
                .to_string_lossy()
                .into_owned(),
            relative_path: relative.to_string_lossy().into_owned(),
            matches: Vec::new(),
        })
    }

    pub fn finish(self) -> FileSearchResult {
        FileSearchResult {
            files: self.files.into_values().collect(),
            total_matches: self.count,
            truncated: self.truncated,
        }
    }
}

fn preview(
    text: &str,
    start: usize,
    end: usize,
    line: u64,
    column: usize,
    match_length: usize,
) -> FileSearchMatch {
    let window_start = text[..start]
        .char_indices()
        .rev()
        .nth(120)
        .map_or(0, |(index, _)| index);
    let window_end = text[window_start..]
        .char_indices()
        .nth(400)
        .map_or(text.len(), |(index, _)| window_start + index);
    let prefix = if window_start > 0 { "…" } else { "" };
    FileSearchMatch {
        line,
        column,
        match_length,
        text: format!(
            "{prefix}{}{}",
            &text[window_start..window_end],
            if window_end < text.len() { "…" } else { "" }
        ),
        preview_column: usize::from(window_start > 0)
            + text[window_start..start].encode_utf16().count(),
        preview_length: text[start..end.min(window_end)].encode_utf16().count(),
    }
}
