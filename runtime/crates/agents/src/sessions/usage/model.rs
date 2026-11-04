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

    pub(super) fn since(self, previous: Self) -> Self {
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

pub(super) fn count(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}
