/// Native columns only; content decoding and timestamp conversion belong to callers.
#[derive(Debug, PartialEq)]
pub struct MessageRecord {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<String>,
    pub timestamp: f64,
    pub finish_reason: Option<String>,
}

/// Flags required to distinguish archived originals, copied contexts and rewound rows.
#[derive(Debug, PartialEq)]
pub struct CompactedMessageRecord {
    pub message: MessageRecord,
    pub active: bool,
    pub compacted: bool,
}
