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
