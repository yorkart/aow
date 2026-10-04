//! Token counts reported by native transcript events, without history backfill.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    pub(crate) fn accumulate(target: &mut Option<Self>, usage: Self) {
        let total = target.get_or_insert_with(Self::default);
        total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
        total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
        total.cached_input_tokens = total
            .cached_input_tokens
            .saturating_add(usage.cached_input_tokens);
        total.cache_write_input_tokens = total
            .cache_write_input_tokens
            .saturating_add(usage.cache_write_input_tokens);
        total.reasoning_output_tokens = total
            .reasoning_output_tokens
            .saturating_add(usage.reasoning_output_tokens);
        total.total_tokens = total.input_tokens.saturating_add(total.output_tokens);
    }

    fn codex(value: &Value) -> Option<Self> {
        let input_tokens = value["input_tokens"].as_u64()?;
        let output_tokens = value["output_tokens"].as_u64()?;
        Some(Self {
            input_tokens,
            output_tokens,
            cached_input_tokens: count(value, "cached_input_tokens"),
            cache_write_input_tokens: count(value, "cache_write_input_tokens"),
            reasoning_output_tokens: count(value, "reasoning_output_tokens"),
            total_tokens: input_tokens.saturating_add(output_tokens),
        })
    }

    fn claude(value: &Value) -> Option<Self> {
        let cached_input_tokens = count(value, "cache_read_input_tokens");
        let cache_write_input_tokens = count(value, "cache_creation_input_tokens");
        let input_tokens = value["input_tokens"]
            .as_u64()?
            .saturating_add(cached_input_tokens)
            .saturating_add(cache_write_input_tokens);
        let output_tokens = value["output_tokens"].as_u64()?;
        Some(Self {
            input_tokens,
            output_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            reasoning_output_tokens: 0,
            total_tokens: input_tokens.saturating_add(output_tokens),
        })
    }

    fn since(self, previous: Self) -> Self {
        let input_tokens = self.input_tokens.saturating_sub(previous.input_tokens);
        let output_tokens = self.output_tokens.saturating_sub(previous.output_tokens);
        Self {
            input_tokens,
            output_tokens,
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_sub(previous.cached_input_tokens),
            cache_write_input_tokens: self
                .cache_write_input_tokens
                .saturating_sub(previous.cache_write_input_tokens),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .saturating_sub(previous.reasoning_output_tokens),
            total_tokens: input_tokens.saturating_add(output_tokens),
        }
    }
}

fn count(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}

#[derive(Default)]
pub(crate) struct CodexUsage {
    // Cumulative counts only identify repeated projections; they are never
    // included in the reported per-turn usage or used to backfill an EOF reader.
    last_total: Option<TokenUsage>,
}

impl CodexUsage {
    pub(crate) fn consume(&mut self, payload: &Value) -> Option<TokenUsage> {
        let info = payload.get("info")?;
        let usage = TokenUsage::codex(&info["last_token_usage"])?;
        let total = TokenUsage::codex(&info["total_token_usage"]);
        if total.is_some() && total == self.last_total {
            return None;
        }
        self.last_total = total;
        Some(usage)
    }
}

#[derive(Default)]
pub(crate) struct ClaudeUsage {
    messages: HashMap<String, TokenUsage>,
}

impl ClaudeUsage {
    pub(crate) fn consume(&mut self, message: &Value) -> Option<TokenUsage> {
        let usage = TokenUsage::claude(&message["usage"])?;
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

#[cfg(test)]
pub(crate) mod tests;
