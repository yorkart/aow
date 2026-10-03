use super::*;
use serde_json::json;
use std::io::Write;

fn fixture(agent: &'static str, records: Vec<Value>) -> (tempfile::TempDir, AgentSessionLocator) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("run.jsonl");
    let mut file = File::create(&path).unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    let locator = AgentSessionLocator {
        agent,
        session_id: "s".into(),
        title: "Review".into(),
        cwd: root.path().into(),
        transcript_path: path,
        trusted_root: root.path().into(),
    };
    (root, locator)
}

fn collect(locator: &AgentSessionLocator) -> Option<String> {
    read_completed_run(locator.clone(), Utc::now() + chrono::Duration::seconds(1)).unwrap()
}

#[test]
fn claude_successful_exit_keeps_every_text_block_after_stop_hooks() {
    let first = "P1: 完整审查意见".repeat(1000);
    for native_stop in [false, true] {
        let mut records = vec![
            json!({"type":"user","message":{"content":"Review"}}),
            json!({"type":"assistant","message":{"id":"old","content":"Old answer"}}),
            json!({"type":"user","isMeta":true,"message":{"content":"Stop hook: continue"}}),
            json!({"type":"assistant","message":{"id":"new","content":[{"type":"text","text":first},{"type":"thinking","thinking":"PRIVATE"}]}}),
            json!({"type":"assistant","message":{"id":"new","stop_reason":"end_turn","content":[{"type":"text","text":"P2\n[AOW_HOSTING_DONE]"}]}}),
        ];
        if native_stop {
            records.push(json!({"type":"system","subtype":"turn_duration"}));
        }
        records
            .push(json!({"type":"assistant","isSidechain":true,"message":{"content":"Unrelated"}}));
        records.push(json!({"type":"last-prompt","sessionId":"s","leafUuid":"unused"}));
        let (_root, locator) = fixture("claude", records);
        assert_eq!(
            collect(&locator).unwrap(),
            format!("{first}\n\nP2\n[AOW_HOSTING_DONE]")
        );
        let bytes = std::fs::read(&locator.transcript_path).unwrap();
        std::fs::write(&locator.transcript_path, &bytes[..bytes.len() - 1]).unwrap();
        assert_eq!(
            collect(&locator).unwrap(),
            format!("{first}\n\nP2\n[AOW_HOSTING_DONE]")
        );
        let finished_before_write = DateTime::<Utc>::from(
            std::fs::metadata(&locator.transcript_path)
                .unwrap()
                .modified()
                .unwrap(),
        ) - chrono::Duration::seconds(1);
        assert!(
            read_completed_run(locator, finished_before_write)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn claude_does_not_return_a_superseded_failed_or_resumed_reply() {
    for invalidation in [
        json!({"type":"user","isMeta":true,"message":{"content":"Continue"}}),
        json!({"type":"user","message":{"content":"Another task"}}),
        json!({"type":"assistant","isApiErrorMessage":true,"message":{"content":"Error"}}),
        json!({"type":"user","interruptedMessageId":"reply"}),
        json!({"type":"system","subtype":"api_error"}),
        json!({"type":"assistant","message":{"id":"tool","stop_reason":"tool_use","content":[{"type":"tool_use","name":"Bash"}]}}),
    ] {
        let (_root, locator) = fixture(
            "claude",
            vec![
                json!({"type":"user","message":{"content":"Review"}}),
                json!({"type":"assistant","message":{"id":"reply","content":"Old reply"}}),
                invalidation,
            ],
        );
        assert!(collect(&locator).is_none());
    }
}

#[test]
fn codex_like_uses_native_completion_and_ignores_trailing_metadata() {
    for agent in ["codex", "traecli"] {
        let (_root, locator) = fixture(
            agent,
            vec![
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":"Review"}}),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"one","last_agent_message":"P1\nP2"}}),
                json!({"type":"event_msg","payload":{"type":"token_count"}}),
            ],
        );
        assert_eq!(collect(&locator).as_deref(), Some("P1\nP2"));
        let exited_at = Utc::now();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&locator.transcript_path)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}})
        )
        .unwrap();
        writeln!(file,"{}",json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"two","last_agent_message":"Later"}})).unwrap();
        assert!(read_completed_run(locator, exited_at).unwrap().is_none());
    }
}

#[test]
fn malformed_results_are_rejected() {
    let (_root, locator) = fixture(
        "claude",
        vec![
            json!({"type":"user","message":{"content":"Review"}}),
            json!({"type":"assistant","message":{"content":"Reply"}}),
        ],
    );
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&locator.transcript_path)
        .unwrap();
    writeln!(file, "{{invalid").unwrap();
    assert!(collect(&locator).is_none());
}

fn read_completed_run(
    locator: AgentSessionLocator,
    exited_at: DateTime<Utc>,
) -> Result<Option<String>, SnapshotError> {
    crate::Agent::from_id(locator.agent)
        .unwrap()
        .session_tracking()
        .unwrap()
        .completed_run_result(&locator, exited_at)
}
