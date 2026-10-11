use super::AcpThread;
use crate::facade::{Permission, SessionSnapshot};
use serde_json::json;

fn thread() -> SessionSnapshot {
    AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    )
}

#[test]
fn protocol_message_ids_preserve_chunk_boundaries() {
    for kind in [
        "user_message_chunk",
        "agent_message_chunk",
        "agent_thought_chunk",
    ] {
        let mut thread = thread();
        for (message_id, chunks) in [
            ("msg_119", ["ACP", "_LIVE", "_", "MAIN", "_DONE"]),
            ("msg_122", ["ACP_L", "IVE_F", "OLLOW", "UP_V", "ISIBLE"]),
        ] {
            for text in chunks {
                AcpThread::handle_session_update(
                    &mut thread,
                    json!({"sessionUpdate":kind,"messageId":message_id,"content":{"type":"text","text":text}}),
                );
            }
            AcpThread::handle_session_update(
                &mut thread,
                json!({"sessionUpdate":"usage_update","used":23773,"size":205200}),
            );
        }
        assert_eq!(thread.entries.len(), 2, "{kind}");
        assert_eq!(thread.entries[0].content["text"], "ACP_LIVE_MAIN_DONE");
        assert_eq!(
            thread.entries[1].content["text"],
            "ACP_LIVE_FOLLOWUP_VISIBLE"
        );
        assert_eq!(thread.entries[0].message_id.as_deref(), Some("msg_119"));
        assert_eq!(thread.entries[1].message_id.as_deref(), Some("msg_122"));
        assert_ne!(thread.entries[0].id, thread.entries[1].id);
    }
}

#[test]
fn missing_message_ids_merge_and_adopt_the_first_known_identity() {
    for kind in [
        "user_message_chunk",
        "agent_message_chunk",
        "agent_thought_chunk",
    ] {
        let mut thread = thread();
        for (message_id, text) in [
            (None, "**Hel"),
            (Some("first"), "lo"),
            (None, "**"),
            (Some("second"), "Follow-up"),
        ] {
            AcpThread::handle_session_update(
                &mut thread,
                json!({"sessionUpdate":kind,"messageId":message_id,"content":{"type":"text","text":text}}),
            );
        }
        assert_eq!(thread.entries.len(), 2, "{kind}");
        assert_eq!(thread.entries[0].content["text"], "**Hello**");
        assert_eq!(thread.entries[0].message_id.as_deref(), Some("first"));
        assert_eq!(thread.entries[1].content["text"], "Follow-up");
    }
}

#[test]
fn legacy_snapshots_without_message_ids_remain_readable() {
    let mut saved = serde_json::to_value(thread()).unwrap();
    saved["entries"] =
        json!([{"id":"local-entry","kind":"assistant","content":{"type":"text","text":"Before"}}]);
    let mut restored: SessionSnapshot = serde_json::from_value(saved).unwrap();
    assert!(restored.entries[0].message_id.is_none());
    AcpThread::handle_session_update(
        &mut restored,
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" after"}}),
    );
    assert_eq!(restored.entries.len(), 1);
    assert_eq!(restored.entries[0].content["text"], "Before after");
    assert_eq!(restored.entries[0].id, "local-entry");
}

#[test]
fn activity_does_not_split_streaming_messages_or_thoughts() {
    for kind in ["agent_message_chunk", "agent_thought_chunk"] {
        let mut thread = thread();
        for update in [
            json!({"sessionUpdate":kind,"content":{"type":"text","text":"**Hel"}}),
            json!({"sessionUpdate":"plan","entries":[{"content":"Read files","status":"pending"}]}),
            json!({"sessionUpdate":"notice","severity":"warning","title":"Rate limit","description":"Wait a moment"}),
            json!({"sessionUpdate":kind,"content":{"type":"text","text":"lo**"}}),
        ] {
            AcpThread::handle_session_update(&mut thread, update);
        }
        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries[0].content["text"], "**Hello**");
        assert_eq!(thread.plan["entries"][0]["content"], "Read files");
        assert_eq!(thread.notices[0].content["description"], "Wait a moment");
    }
}

#[test]
fn permission_upserts_tool_preview_without_losing_existing_fields() {
    for existing in [false, true] {
        let mut thread = thread();
        if existing {
            AcpThread::handle_session_update(
                &mut thread,
                json!({"sessionUpdate":"tool_call","toolCallId":"edit","status":"cancelled","title":"Old title","rawInput":{"path":"/repo/main.rs"}}),
            );
        }
        AcpThread::request_permission(
            &mut thread,
            Permission {
                id: "permission".into(),
                kind: "permission".into(),
                request: json!({"toolCall":{"toolCallId":"edit","title":"Review changes","kind":"edit","content":[{"type":"diff","path":"/repo/main.rs","oldText":"old","newText":"new"}]},"options":[]}),
            },
        );
        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries[0].id, "edit");
        assert_eq!(thread.entries[0].content["title"], "Review changes");
        assert_eq!(thread.entries[0].content["content"][0]["newText"], "new");
        if existing {
            assert_eq!(
                thread.entries[0].content["rawInput"]["path"],
                "/repo/main.rs"
            );
        }
        assert_eq!(thread.permissions.len(), 1);
    }
}

#[test]
fn compaction_retains_one_entry_and_applies_patch_semantics() {
    let mut thread = thread();
    let update =
        |thread: &mut SessionSnapshot, value| AcpThread::handle_session_update(thread, value);
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"in_progress"}),
    );
    for text in ["**Hel", "lo**"] {
        update(
            &mut thread,
            json!({"sessionUpdate":"compaction_summary_chunk","compactionId":"c","content":{"type":"text","text":text}}),
        );
    }
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"failed","error":"Out of space"}),
    );
    assert_eq!(thread.entries.len(), 1);
    assert_eq!(thread.entries[0].content["summary"][0]["text"], "**Hello**");
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_summary_chunk","compactionId":"c","content":{"type":"text","text":"ignored after completion"}}),
    );
    assert_eq!(thread.entries[0].content["summary"][0]["text"], "**Hello**");
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"completed","summary":[{"type":"text","text":"Replacement"}],"error":null}),
    );
    assert_eq!(
        thread.entries[0].content["summary"][0]["text"],
        "Replacement"
    );
    assert!(thread.entries[0].content["error"].is_null());
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"completed","summary":null}),
    );
    assert_eq!(thread.entries[0].content["summary"], json!([]));
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_summary_chunk","compactionId":"unknown","content":{"type":"text","text":"ignored"}}),
    );
    assert_eq!(thread.entries.len(), 1);
    update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"next","status":"in_progress"}),
    );
    assert_eq!(thread.entries.len(), 2);
}
