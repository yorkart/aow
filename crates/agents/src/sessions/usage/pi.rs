use serde_json::Value;

use super::{TokenUsage, TokenUsageParser, model::count};

#[derive(Default)]
pub(crate) struct PiUsage;

impl TokenUsageParser for PiUsage {
    fn consume(&mut self, record: &Value) -> Option<TokenUsage> {
        let value = &record["usage"];
        let cached_input_tokens = count(value, "cacheRead");
        let cache_write_input_tokens = count(value, "cacheWrite");
        let input_tokens = value["input"]
            .as_u64()?
            .saturating_add(cached_input_tokens)
            .saturating_add(cache_write_input_tokens);
        let output_tokens = value["output"].as_u64()?;
        Some(TokenUsage {
            input_tokens,
            output_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            reasoning_output_tokens: 0,
            total_tokens: input_tokens.saturating_add(output_tokens),
        })
    }
}
