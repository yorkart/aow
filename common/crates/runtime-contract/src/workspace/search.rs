use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileSearchMode {
    Name,
    Content,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileSearchRequest {
    pub root: String,
    pub query: String,
    pub mode: FileSearchMode,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub use_regex: bool,
    #[serde(default)]
    pub include_ignored: bool,
    #[serde(default)]
    pub include_pattern: String,
    #[serde(default)]
    pub exclude_pattern: String,
}

/// Columns and lengths use UTF-16 code units, as in Monaco and JavaScript.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileSearchMatch {
    pub line: u64,
    pub column: usize,
    pub match_length: usize,
    pub text: String,
    pub preview_column: usize,
    pub preview_length: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileSearchFile {
    pub path: String,
    pub relative_path: String,
    pub matches: Vec<FileSearchMatch>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct FileSearchResult {
    pub files: Vec<FileSearchFile>,
    pub total_matches: usize,
    pub truncated: bool,
}
