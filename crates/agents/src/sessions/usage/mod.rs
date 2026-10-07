//! Token counts reported by native transcript events, without history backfill.

mod claude;
mod codex;
mod model;
mod pi;

use serde_json::Value;

pub(crate) use claude::ClaudeUsage;
pub(crate) use codex::CodexUsage;
pub use model::TokenUsage;
pub(crate) use pi::PiUsage;

/// Per-reader accounting of native usage-bearing messages or event payloads.
/// Implementations own field mapping and deduplication; callers own turn boundaries.
pub(crate) trait TokenUsageParser {
    /// Return newly observed counts, or None for missing/invalid usage.
    /// Repeated projections must not add counts; explicit zero remains known usage.
    fn consume(&mut self, record: &Value) -> Option<TokenUsage>;
}

#[cfg(test)]
pub(crate) mod tests;
