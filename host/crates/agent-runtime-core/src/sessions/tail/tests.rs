use super::*;
use std::{fs::OpenOptions, io::Write, path::Path};

fn locator(root: &Path, agent: &'static str) -> AgentSessionLocator {
    AgentSessionLocator {
        agent,
        session_id: "session".into(),
        title: "Task".into(),
        cwd: root.into(),
        transcript_path: root.join("session.jsonl"),
        trusted_root: root.into(),
    }
}

fn append(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

const COMPLETE: &[u8] =
    b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\"}}\n";

#[test]
fn usage_counts_only_eof_events_and_is_frozen_and_reset_at_each_completion() {
    use crate::sessions::usage::tests::codex_event;
    use serde_json::json;
    for agent in ["codex", "traecli"] {
        let root = tempfile::tempdir().unwrap();
        let locator = locator(root.path(), agent);
        let historical = codex_event(1_000, 100, 1_000, 100);
        std::fs::write(&locator.transcript_path, format!("{historical}\n")).unwrap();
        let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
        let first = codex_event(100, 20, 1_100, 120);
        append(&locator.transcript_path, format!("{first}\n").as_bytes());
        assert!(tail.poll().unwrap().is_empty());
        // Additional user input and tools must not clear earlier calls in this task.
        for event in [
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":"continue"}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command"}}),
            first,
            codex_event(200, 30, 1_300, 150),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}),
            codex_event(10, 2, 1_310, 152),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-2"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-3"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-3"}}),
        ] {
            append(&locator.transcript_path, format!("{event}\n").as_bytes());
        }
        let events = tail.poll().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].usage.unwrap().total_tokens, 350);
        assert_eq!(events[1].usage.unwrap().total_tokens, 12);
        assert_eq!(events[2].usage, None);
        assert!(tail.poll().unwrap().is_empty());
    }
}

#[test]
fn claude_usage_survives_tools_and_is_reset_after_the_main_loop_boundary() {
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "claude");
    std::fs::write(&locator.transcript_path, "").unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    for event in [
        json!({"type":"assistant","isSidechain":true,"message":{"id":"side","usage":{"input_tokens":999,"output_tokens":99}}}),
        json!({"type":"assistant","message":{"id":"tool","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10},"content":[{"type":"tool_use"}]}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result"}]}}),
        json!({"type":"assistant","message":{"id":"final","usage":{"input_tokens":200,"output_tokens":20},"content":[{"type":"text","text":"Done"}]}}),
        json!({"type":"assistant","message":{"id":"final","usage":{"input_tokens":200,"output_tokens":20},"content":[]}}),
        json!({"type":"system","subtype":"turn_duration","uuid":"first"}),
        json!({"type":"system","subtype":"turn_duration","uuid":"second"}),
    ] {
        append(&locator.transcript_path, format!("{event}\n").as_bytes());
    }
    let events = tail.poll().unwrap();
    assert_eq!(events[0].usage.unwrap().total_tokens, 330);
    assert_eq!(events[0].conclusion.as_deref(), Some("Done"));
    assert_eq!(events[1].usage, None);
}

#[test]
fn starts_at_eof_and_consumes_each_appended_record_once() {
    for agent in ["codex", "traecli"] {
        let root = tempfile::tempdir().unwrap();
        let locator = locator(root.path(), agent);
        std::fs::write(&locator.transcript_path, COMPLETE).unwrap();
        let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
        assert!(tail.poll().unwrap().is_empty());
        append(&locator.transcript_path, COMPLETE);
        let events = tail.poll().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].turn_id.as_deref(), Some("turn-1"));
        assert!(tail.poll().unwrap().is_empty());
        append(&locator.transcript_path, COMPLETE);
        assert_eq!(tail.poll().unwrap().len(), 1); // no session/turn dedup layer
    }
}

#[test]
fn partial_records_wait_for_newline_and_preexisting_partial_record_is_skipped() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "codex");
    std::fs::write(&locator.transcript_path, &COMPLETE[..20]).unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    append(&locator.transcript_path, &COMPLETE[20..]);
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, &COMPLETE[..20]);
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, &COMPLETE[20..]);
    assert_eq!(tail.poll().unwrap().len(), 1);
}

#[test]
fn ignores_tools_approval_requests_errors_and_intermediate_messages() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "codex");
    std::fs::write(&locator.transcript_path, "").unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    for kind in [
        "task_started",
        "item_completed",
        "approval_requested",
        "error",
        "agent_message",
        "turn_aborted",
    ] {
        append(
            &locator.transcript_path,
            format!("{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"{kind}\"}}}}\n").as_bytes(),
        );
    }
    append(&locator.transcript_path, b"not json\n");
    assert!(tail.poll().unwrap().is_empty());
    append(
        &locator.transcript_path,
        b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"turn_complete\"}}\n",
    );
    assert_eq!(tail.poll().unwrap().len(), 1);
}

#[test]
fn claude_uses_the_main_loop_boundary_not_assistant_stop_reason() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "claude");
    std::fs::write(&locator.transcript_path, "").unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    for record in [
        serde_json::json!({"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"暂时结束"}]}}),
        serde_json::json!({"type":"system","subtype":"turn_duration","sessionId":"other"}),
        serde_json::json!({"type":"system","subtype":"turn_duration","sessionId":"session","isSidechain":true}),
    ] {
        append(&locator.transcript_path, format!("{record}\n").as_bytes());
    }
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, b"{\"type\":\"system\",\"subtype\":\"turn_duration\",\"sessionId\":\"session\",\"uuid\":\"end-1\"}\n");
    let events = tail.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].turn_id.as_deref(), Some("end-1"));
    assert_eq!(events[0].conclusion.as_deref(), Some("暂时结束"));
}

#[test]
fn conclusion_survives_poll_boundaries_but_not_history_or_skipped_records() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "codex");
    let conclusion = "完整结论中文🦀\n".repeat(6000);
    let reply = serde_json::json!({"type":"event_msg","payload":{"type":"agent_message","phase":"final_answer","message":conclusion}});
    let reply = format!("{reply}\n");
    std::fs::write(&locator.transcript_path, &reply).unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    append(&locator.transcript_path, COMPLETE);
    assert_eq!(tail.poll().unwrap()[0].conclusion, None);
    append(&locator.transcript_path, &reply.as_bytes()[..30]);
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, &reply.as_bytes()[30..]);
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, COMPLETE);
    assert_eq!(
        tail.poll().unwrap()[0].conclusion.as_deref(),
        Some(conclusion.as_str())
    );
    for skipped in [
        b"bad json\n".to_vec(),
        [vec![b'x'; MAX_LINE + 10], b"\n".to_vec()].concat(),
    ] {
        append(&locator.transcript_path, reply.as_bytes());
        assert!(tail.poll().unwrap().is_empty());
        append(&locator.transcript_path, &skipped);
        append(&locator.transcript_path, COMPLETE);
        let mut events = tail.poll().unwrap();
        events.extend(tail.poll().unwrap());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].conclusion, None);
    }
}

#[test]
fn truncation_and_replacement_attach_at_eof_without_replaying() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "codex");
    std::fs::write(&locator.transcript_path, [COMPLETE, COMPLETE].concat()).unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    std::fs::write(&locator.transcript_path, COMPLETE).unwrap();
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, COMPLETE);
    assert_eq!(tail.poll().unwrap().len(), 1);
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, [COMPLETE, COMPLETE, COMPLETE].concat()).unwrap();
    std::fs::rename(replacement, &locator.transcript_path).unwrap();
    assert!(tail.poll().unwrap().is_empty());
    append(&locator.transcript_path, COMPLETE);
    assert_eq!(tail.poll().unwrap().len(), 1);
}

#[test]
fn read_budget_and_oversized_lines_do_not_hide_following_stop_records() {
    let root = tempfile::tempdir().unwrap();
    let locator = locator(root.path(), "codex");
    std::fs::write(&locator.transcript_path, "").unwrap();
    let mut tail = SessionTail::from_eof(locator.clone()).unwrap();
    append(&locator.transcript_path, &vec![b'x'; MAX_LINE + 100]);
    append(&locator.transcript_path, b"\n");
    append(&locator.transcript_path, COMPLETE);
    assert!(tail.poll().unwrap().is_empty());
    assert_eq!(tail.poll().unwrap().len(), 1);
    assert!(tail.poll().unwrap().is_empty());
}

#[test]
fn rejects_transcripts_outside_the_history_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut locator = locator(root.path(), "codex");
    locator.transcript_path = outside.path().join("session.jsonl");
    std::fs::write(&locator.transcript_path, "").unwrap();
    assert!(SessionTail::from_eof(locator).is_err());
}
