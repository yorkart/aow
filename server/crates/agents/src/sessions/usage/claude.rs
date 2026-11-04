use std::collections::HashMap;

use serde_json::Value;

use super::{TokenUsage, TokenUsageParser, model::count};

#[derive(Default)]
pub(crate) struct ClaudeUsage {
    messages: HashMap<String, TokenUsage>,
}

impl TokenUsageParser for ClaudeUsage {
    fn consume(&mut self, message: &Value) -> Option<TokenUsage> {
        let usage = parse(&message["usage"])?;
        let Some(id) = message["id"].as_str() else {
            return Some(usage);
        };
        let previous = self.messages.entry(id.to_owned()).or_default();
        let delta = usage.since(*previous);
        // Keep the largest counts if content-block projections arrive out of order.
        previous.input_tokens = previous.input_tokens.max(usage.input_tokens);
        previous.output_tokens = previous.output_tokens.max(usage.output_tokens);
        previous.cached_input_tokens = previous.cached_input_tokens.max(usage.cached_input_tokens);
        previous.cache_write_input_tokens = previous
            .cache_write_input_tokens
            .max(usage.cache_write_input_tokens);
        previous.total_tokens = previous.input_tokens.saturating_add(previous.output_tokens);
        Some(delta)
    }
}

fn parse(value: &Value) -> Option<TokenUsage> {
    let cached_input_tokens = count(value, "cache_read_input_tokens");
    let cache_write_input_tokens = count(value, "cache_creation_input_tokens");
    let input_tokens = value["input_tokens"]
        .as_u64()?
        .saturating_add(cached_input_tokens)
        .saturating_add(cache_write_input_tokens);
    let output_tokens = value["output_tokens"].as_u64()?;
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
        cache_write_input_tokens,
        reasoning_output_tokens: 0,
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}
