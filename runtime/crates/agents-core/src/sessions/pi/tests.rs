use super::*;
use crate::sessions::tracking::{
    AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget,
};
use serde_json::json;
use std::{fs, io::Write};

fn fixture(records: &[Value]) -> (tempfile::TempDir, SessionRoots, AgentSessionLocator) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sessions");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("2026_session-one.jsonl");
    let mut file = File::create(&path).unwrap();
    writeln!(file, "{}", json!({"type":"session","version":3,"id":"session-one","cwd":"/work/repo","timestamp":"2026-10-01T00:00:00Z"})).unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    let roots = SessionRoots::from_configuration(
        directory.path(),
        &SessionEnvironment::from([("PI_CODING_AGENT_SESSION_DIR".into(), root)]),
    );
    let locator = Pi.find_session(&roots, "session-one").unwrap().locator();
    (directory, roots, locator)
}

fn user(id: &str, parent: Option<&str>, text: &str) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"message":{"role":"user","content":text}})
}

fn reply(id: &str, parent: &str, reason: &str, text: &str) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"message":{"role":"assistant","stopReason":reason,
        "content":[{"type":"thinking","thinking":"private reasoning"},{"type":"text","text":text}],
        "usage":{"input":10,"output":5,"cacheRead":3,"cacheWrite":2}}})
}

fn settled(id: &str, parent: &str) -> Value {
    json!({"type":"custom","id":id,"parentId":parent,"customType":"aow.pi","data":{"event":"settled","session_id":"session-one"}})
}

#[test]
fn native_history_selects_the_current_branch_and_name_without_exposing_reasoning() {
    let (_dir, roots, locator) = fixture(&[
        user("u1", None, "First request"),
        reply("a1", "u1", "stop", "Old reply"),
        user("discarded", Some("a1"), "Discarded request"),
        reply("discarded-a", "discarded", "stop", "Discarded reply"),
        user("u2", Some("a1"), "Active request"),
        reply("a2", "u2", "stop", "Active reply"),
        json!({"type":"session_info","id":"name","parentId":"a2","name":"Named session"}),
    ]);
    assert_eq!(locator.title, "Named session");
    assert_eq!(Pi.list_sessions(&roots, Path::new("/work/repo")).len(), 1);
    assert!(
        Pi.list_sessions(&roots, Path::new("/work/repository"))
            .is_empty()
    );
    assert!(Pi.find_session(&roots, "session").is_none());
    let snapshot = serde_json::to_value(Pi.read_snapshot(locator).unwrap()).unwrap();
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["turns"][1]["final"]["text"], "Active reply");
    assert_eq!(snapshot["turns"][1]["usage"]["input_tokens"], 15);
    assert_eq!(snapshot["turns"][1]["usage"]["total_tokens"], 20);
    assert!(!snapshot.to_string().contains("private reasoning"));
    assert!(!snapshot.to_string().contains("Discarded"));
}

#[test]
fn snapshots_retain_tools_compaction_and_final_response_in_order() {
    let mut tool = reply("a1", "u1", "toolUse", "Checking files");
    tool["message"]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"toolCall","id":"call","name":"bash","arguments":{"command":"pwd"}}));
    let (_dir, _, locator) = fixture(&[
        user("u1", None, "Review"),
        tool,
        json!({"type":"message","id":"result","parentId":"a1","message":{"role":"toolResult","toolCallId":"call","toolName":"bash","content":[{"type":"text","text":"/work/repo"}],"isError":false}}),
        json!({"type":"compaction","id":"compact","parentId":"result","summary":"Summary","firstKeptEntryId":"u1","tokensBefore":100}),
        reply("a2", "compact", "stop", "Done"),
    ]);
    let snapshot = serde_json::to_value(Pi.read_snapshot(locator).unwrap()).unwrap();
    let turn = &snapshot["turns"][0];
    assert_eq!(turn["final"]["text"], "Done");
    assert_eq!(turn["activities"][0]["text"], "Checking files");
    assert_eq!(turn["activities"][1]["text"], "bash");
    assert_eq!(turn["activities"][1]["details"]["command"]["text"], "pwd");
    assert_eq!(
        turn["activities"][1]["details"]["output"]["text"],
        "/work/repo"
    );
    assert_eq!(turn["usage"]["total_tokens"], 40);
}

#[test]
fn stop_events_require_settlement_and_do_not_reuse_failed_or_previous_replies() {
    let mut parser = Agent::Pi.session_tracking().unwrap().task_stop_parser();
    assert!(
        parser
            .consume("session-one", &user("u1", None, "Review"))
            .is_none()
    );
    assert!(
        parser
            .consume("session-one", &reply("a1", "u1", "stop", "Provisional"))
            .is_none()
    );
    assert!(
        parser
            .consume("session-one", &reply("a2", "a1", "error", "Failed"))
            .is_none()
    );
    assert!(
        parser
            .consume("session-one", &settled("s1", "a2"))
            .is_none()
    );
    parser.consume("session-one", &reply("a3", "a2", "stop", "Recovered"));
    assert!(parser.consume("other", &settled("s2", "a3")).is_none());
    let event = parser.consume("session-one", &settled("s2", "a3")).unwrap();
    assert_eq!(event.conclusion.as_deref(), Some("Recovered"));
    assert_eq!(event.usage.unwrap().total_tokens, 60);
    assert!(
        parser
            .consume("session-one", &settled("s2", "a3"))
            .is_none()
    );
    parser.consume("session-one", &user("u2", Some("s2"), "Next"));
    assert!(
        parser
            .consume("session-one", &settled("s3", "u2"))
            .is_none()
    );
    assert!(parser.take_conclusion().is_none());
}

#[test]
fn completed_print_result_follows_the_active_branch_and_rejects_unfinished_work() {
    for (reason, expected) in [
        ("stop", Some("Answer")),
        ("error", None),
        ("aborted", None),
        ("toolUse", None),
    ] {
        let (_dir, _, locator) =
            fixture(&[user("u", None, "Review"), reply("a", "u", reason, "Answer")]);
        let result = Agent::Pi
            .session_tracking()
            .unwrap()
            .completed_run_result(&locator, Utc::now())
            .unwrap();
        assert_eq!(result.as_deref(), expected);
    }
    let (_dir, _, locator) = fixture(&[
        user("u1", None, "Old"),
        reply("a1", "u1", "stop", "Old answer"),
        settled("s", "a1"),
        user("u2", Some("s"), "Pending"),
    ]);
    assert!(
        Agent::Pi
            .session_tracking()
            .unwrap()
            .completed_run_result(&locator, Utc::now())
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn live_identity_requires_the_exact_process_and_updates_after_session_switches() {
    let (directory, _, _) = fixture(&[]);
    let binding = directory.path().join("binding.json");
    let environment = SessionEnvironment::from([("AOW_PI_BINDING".into(), binding.clone())]);
    let tracker = Agent::Pi.session_tracking().unwrap();
    for id in ["first", "second"] {
        fs::write(
            &binding,
            json!({"pid":42,"cwd":"/work/repo","session_id":id}).to_string(),
        )
        .unwrap();
        let context = || LiveSessionContext {
            pid: Some(42),
            cwd: "/work/repo",
            title: "Pi - repo",
            environment: &environment,
        };
        assert_eq!(
            tracker.resolve_live_session(context()).await,
            SessionResolution::Resolved(SessionTarget::Id(id.into()))
        );
        assert_eq!(
            tracker
                .resolve_live_session(LiveSessionContext {
                    pid: Some(43),
                    ..context()
                })
                .await,
            SessionResolution::NotFound
        );
        assert_eq!(
            tracker
                .resolve_live_session(LiveSessionContext {
                    cwd: "/other",
                    ..context()
                })
                .await,
            SessionResolution::NotFound
        );
    }
}

#[test]
fn roots_honor_native_overrides_and_older_saved_roots_remain_readable() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    assert_eq!(
        Pi.session_root(home, &SessionEnvironment::new()),
        home.join(".pi/agent/sessions")
    );
    let mut env = SessionEnvironment::from([("PI_CODING_AGENT_DIR".into(), home.join("profile"))]);
    fs::create_dir_all(home.join("profile")).unwrap();
    fs::write(
        home.join("profile/settings.json"),
        r#"{"sessionDir":"~/history"}"#,
    )
    .unwrap();
    assert_eq!(Pi.session_root(home, &env), home.join("history"));
    env.insert("PI_CODING_AGENT_SESSION_DIR".into(), home.join("override"));
    assert_eq!(Pi.session_root(home, &env), home.join("override"));
    let old: SessionRoots =
        serde_json::from_value(json!({"codex":"/c","claude":"/a","traecli":"/t","hermes":"/h"}))
            .unwrap();
    assert!(Pi.find_session(&old, "missing").is_none());
}

#[test]
fn malformed_ancestry_and_identity_changes_do_not_produce_snapshots() {
    for records in [
        vec![user("u", Some("missing"), "Review")],
        vec![
            user("u", Some("a"), "Review"),
            reply("a", "u", "stop", "Cycle"),
        ],
        vec![user("u", None, "One"), user("u", None, "Duplicate")],
    ] {
        let (_dir, _, locator) = fixture(&records);
        assert!(Pi.read_snapshot(locator).is_err());
    }
    let (_dir, _, mut locator) = fixture(&[user("u", None, "Review")]);
    locator.session_id = "wrong".into();
    assert!(Pi.read_snapshot(locator).is_err());
}
