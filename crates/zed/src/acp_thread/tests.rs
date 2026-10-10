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

#[test]
fn old_history_moves_activity_and_merges_compaction_updates() {
    let mut thread = thread();
    for (kind, content) in [
        ("assistant", json!({"type":"text","text":"**Hel"})),
        ("plan", json!({"entries":[]})),
        ("notice", json!({"title":"Notice"})),
        ("assistant", json!({"type":"text","text":"lo**"})),
        (
            "compaction_update",
            json!({"compactionId":"c","status":"in_progress"}),
        ),
        (
            "compaction_update",
            json!({"compactionId":"c","status":"completed","summary":[{"type":"text","text":"Summary"}]}),
        ),
    ] {
        thread.entries.push(crate::facade::ThreadEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            content,
        });
    }
    // Additive defaults also support snapshots predating these fields.
    let mut value = serde_json::to_value(&thread).unwrap();
    value.as_object_mut().unwrap().remove("plan");
    value.as_object_mut().unwrap().remove("notices");
    let mut restored: SessionSnapshot = serde_json::from_value(value).unwrap();
    AcpThread::restore_history(&mut restored);
    assert_eq!(restored.entries.len(), 2);
    assert_eq!(restored.entries[0].content["text"], "**Hello**");
    assert_eq!(restored.entries[1].content["status"], "completed");
    assert_eq!(restored.entries[1].id, thread.entries[4].id);
    assert_eq!(restored.notices.len(), 1);
    assert_eq!(restored.notices[0].id, thread.entries[2].id);
}

#[test]
fn restoring_current_snapshots_preserves_message_blocks_and_entry_ids() {
    let mut thread = thread();
    for kind in ["user", "user", "assistant", "assistant"] {
        AcpThread::push(
            &mut thread,
            kind,
            json!({"type":"text","text":"separate content block"}),
        );
    }
    AcpThread::handle_session_update(
        &mut thread,
        json!({"sessionUpdate":"compaction_update","compactionId":"c","status":"completed"}),
    );
    let entries = serde_json::to_value(&thread.entries).unwrap();
    AcpThread::restore_history(&mut thread);
    assert_eq!(serde_json::to_value(&thread.entries).unwrap(), entries);
}
