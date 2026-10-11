use serde_json::Value;

use super::{TokenUsage, TokenUsageParser, model::count};

/// Shared by Codex and TraeCode's compatible token-count protocol.
#[derive(Default)]
pub(crate) struct CodexUsage {
    // Cumulative counts only identify repeated projections; they are never
    // included in the reported per-turn usage or used to backfill an EOF reader.
    last_total: Option<TokenUsage>,
}

impl TokenUsageParser for CodexUsage {
    fn consume(&mut self, payload: &Value) -> Option<TokenUsage> {
        let info = payload.get("info")?;
        let usage = parse(&info["last_token_usage"])?;
        let total = parse(&info["total_token_usage"]);
        if total.is_some() && total == self.last_total {
            return None;
        }
        self.last_total = total;
        Some(usage)
    }
}

fn parse(value: &Value) -> Option<TokenUsage> {
    let input_tokens = value["input_tokens"].as_u64()?;
    let output_tokens = value["output_tokens"].as_u64()?;
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: count(value, "cached_input_tokens"),
        cache_write_input_tokens: count(value, "cache_write_input_tokens"),
        reasoning_output_tokens: count(value, "reasoning_output_tokens"),
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}
