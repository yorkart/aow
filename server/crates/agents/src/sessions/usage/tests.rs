use super::*;
use serde_json::json;

pub(crate) fn codex_event(input: u64, output: u64, total_input: u64, total_output: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{
        "last_token_usage":{"input_tokens":input,"output_tokens":output,"cached_input_tokens":input / 2,"reasoning_output_tokens":output / 2},
        "total_token_usage":{"input_tokens":total_input,"output_tokens":total_output}
    }}})
}

#[test]
fn codex_uses_only_observed_calls_and_ignores_repeated_cumulative_projections() {
    let mut events = CodexUsage::default();
    let mut usage = None;
    let first = codex_event(100, 20, 10_100, 1_020);
    TokenUsage::accumulate(&mut usage, events.consume(&first["payload"]).unwrap());
    assert!(events.consume(&first["payload"]).is_none());
    let second = codex_event(200, 30, 10_300, 1_050);
    TokenUsage::accumulate(&mut usage, events.consume(&second["payload"]).unwrap());
    let usage = usage.unwrap();
    assert_eq!(usage.input_tokens, 300);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.cached_input_tokens, 150);
    assert_eq!(usage.reasoning_output_tokens, 25);
    assert_eq!(usage.total_tokens, 350);
}

#[test]
fn missing_and_invalid_codex_usage_is_unknown_but_explicit_zero_is_known() {
    let mut events = CodexUsage::default();
    for payload in [
        json!({}),
        json!({"info":null}),
        json!({"info":{"last_token_usage":{"input_tokens":-1,"output_tokens":2}}}),
    ] {
        assert!(events.consume(&payload).is_none());
    }
    assert_eq!(
        events.consume(&codex_event(0, 0, 0, 0)["payload"]),
        Some(TokenUsage::default())
    );
}

#[test]
fn claude_counts_cache_tokens_once_and_deduplicates_content_block_usage() {
    let mut events = ClaudeUsage::default();
    let mut usage = None;
    let first = json!({"id":"message-1","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}});
    TokenUsage::accumulate(&mut usage, events.consume(&first).unwrap());
    TokenUsage::accumulate(&mut usage, events.consume(&first).unwrap());
    let mut updated = first.clone();
    updated["usage"]["output_tokens"] = json!(8);
    TokenUsage::accumulate(&mut usage, events.consume(&updated).unwrap());
    TokenUsage::accumulate(&mut usage, events.consume(&first).unwrap());
    let next = json!({"id":"message-2","usage":{"input_tokens":2,"output_tokens":3}});
    TokenUsage::accumulate(&mut usage, events.consume(&next).unwrap());
    assert_eq!(
        usage.unwrap(),
        TokenUsage {
            input_tokens: 132,
            output_tokens: 11,
            cached_input_tokens: 100,
            cache_write_input_tokens: 20,
            reasoning_output_tokens: 0,
            total_tokens: 143,
        }
    );
}

#[test]
fn pi_reports_each_calls_cache_inclusive_usage_and_distinguishes_unknown_from_zero() {
    let mut events = PiUsage;
    for record in [
        json!({}),
        json!({"usage":null}),
        json!({"usage":{"input":-1,"output":2}}),
        json!({"usage":{"input":10}}),
    ] {
        assert!(events.consume(&record).is_none());
    }
    assert_eq!(
        events.consume(&json!({"usage":{"input":0,"output":0}})),
        Some(TokenUsage::default())
    );
    let call = json!({"usage":{"input":10,"output":5,"cacheRead":100,"cacheWrite":20}});
    let mut usage = None;
    // Equal counts from distinct Pi calls are not cumulative projections.
    for _ in 0..2 {
        TokenUsage::accumulate(&mut usage, events.consume(&call).unwrap());
    }
    assert_eq!(
        usage.unwrap(),
        TokenUsage {
            input_tokens: 260,
            output_tokens: 10,
            cached_input_tokens: 200,
            cache_write_input_tokens: 40,
            reasoning_output_tokens: 0,
            total_tokens: 270,
        }
    );
}
