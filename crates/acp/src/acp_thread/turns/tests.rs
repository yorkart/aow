use super::*;
use crate::acp_thread::{AcpThread, history::ThreadStore};
use serde_json::json;

fn snapshot() -> SessionSnapshot {
    AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    )
}

fn message(thread: &mut SessionSnapshot, id: Option<&str>, value: &str) {
    AcpThread::handle_session_update(
        thread,
        json!({
            "sessionUpdate": "agent_message_chunk", "messageId": id,
            "content": {"type": "text", "text": value}
        }),
    );
}

#[test]
fn final_selection_uses_complete_message_boundaries_and_retains_legacy_text() {
    let mut thread = snapshot();
    message(&mut thread, Some("progress"), "Checking files");
    message(&mut thread, Some("final"), "**All ");
    message(&mut thread, Some("final"), "done**");
    let final_entries = final_message(&thread.entries);
    assert_eq!(final_entries.len(), 1);
    assert_eq!(final_entries[0].content["text"], "**All done**");
    let mut legacy = snapshot();
    message(&mut legacy, None, "Progress without a boundary. ");
    message(&mut legacy, None, "Conclusion.");
    assert_eq!(
        final_message(&legacy.entries)[0].content["text"],
        "Progress without a boundary. Conclusion."
    );
    for kind in ["tool", "thought", "compaction_update"] {
        let mut ended = thread.clone();
        AcpThread::push(
            &mut ended,
            kind,
            json!({"type":"text","text":"Later activity"}),
        );
        assert!(
            final_message(&ended.entries).is_empty(),
            "{kind} must not promote earlier progress"
        );
    }
}

#[test]
fn prompt_outcomes_survive_new_turns_and_disk_reload() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    store.insert(snapshot())?;
    for (reason, error, status) in [
        (Some("end_turn"), None, "completed"),
        (Some("cancelled"), None, "interrupted"),
        (Some("max_tokens"), None, "interrupted"),
        (None, Some("Adapter failed"), "failed"),
    ] {
        store.update("local", |thread| {
            let entry_start = thread.entries.len();
            thread.status = "working".into();
            thread.stop_reason = None;
            thread.error = None;
            AcpThread::push(thread, "user", json!({"type":"text","text":"Prompt"}));
            start(thread, entry_start);
            message(thread, None, "Response");
        })?;
        store.update("local", |thread| {
            thread.status = "idle".into();
            thread.stop_reason = reason.map(str::to_owned);
            thread.error = error.map(str::to_owned);
        })?;
        assert_eq!(
            store.snapshot("local")?.turns.last().unwrap().status,
            status
        );
    }
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()))?;
    let restored = restored.snapshot("local")?;
    assert_eq!(
        restored
            .turns
            .iter()
            .map(|turn| turn.status.as_str())
            .collect::<Vec<_>>(),
        ["completed", "interrupted", "interrupted", "failed"]
    );
    assert_eq!(restored.turns[3].error.as_deref(), Some("Adapter failed"));
    Ok(())
}

#[test]
fn unfinished_turns_are_interrupted_on_reload_and_legacy_snapshots_still_load() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()))?;
    let mut thread = snapshot();
    thread.status = "working".into();
    AcpThread::push(&mut thread, "user", json!({"type":"text","text":"Prompt"}));
    start(&mut thread, 0);
    store.insert(thread)?;
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()))?;
    assert_eq!(restored.snapshot("local")?.turns[0].status, "interrupted");
    let mut legacy = serde_json::to_value(snapshot())?;
    legacy.as_object_mut().unwrap().remove("turns");
    assert!(
        serde_json::from_value::<SessionSnapshot>(legacy)?
            .turns
            .is_empty()
    );
    Ok(())
}

#[test]
fn replay_preserves_outcomes_only_when_the_recorded_transcript_matches() {
    let mut previous = snapshot();
    AcpThread::push(
        &mut previous,
        "user",
        json!({"type":"text","text":"Same prompt"}),
    );
    start(&mut previous, 0);
    message(&mut previous, Some("reply"), "Partial response");
    previous.stop_reason = Some("cancelled".into());
    finish(&mut previous);
    let mut replay = snapshot();
    AcpThread::push(
        &mut replay,
        "user",
        json!({"type":"text","text":"Same prompt"}),
    );
    message(&mut replay, Some("reply"), "Partial response");
    restore(&previous, &mut replay);
    assert_eq!(replay.turns[0].user_entry_id, replay.entries[0].id);
    assert_eq!(replay.turns[0].status, "interrupted");
    replay.turns.clear();
    replay.entries[1].content["text"] = "Different result".into();
    restore(&previous, &mut replay);
    assert!(replay.turns.is_empty());
}
