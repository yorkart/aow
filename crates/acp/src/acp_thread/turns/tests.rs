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
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
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
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
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
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    let mut thread = snapshot();
    thread.status = "working".into();
    AcpThread::push(&mut thread, "user", json!({"type":"text","text":"Prompt"}));
    start(&mut thread, 0);
    store.insert(thread)?;
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
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

fn steering_replay() -> (SessionSnapshot, SessionSnapshot) {
    let mut previous = snapshot();
    AcpThread::push(&mut previous, "user", json!({"type":"text", "text":"Task"}));
    start(&mut previous, 0);
    message(&mut previous, Some("answer"), "First ");
    for (text, status) in [("Correction", "injected"), ("Unsent", "failed")] {
        AcpThread::push(
            &mut previous,
            "steering",
            json!({"type":"text", "text":text}),
        );
        previous.entries.last_mut().unwrap().delivery = Some(crate::facade::InputDelivery {
            status: status.into(),
            error: None,
        });
    }
    message(&mut previous, Some("answer"), "answer");
    AcpThread::push(
        &mut previous,
        "tool",
        json!({"title":"Inspect", "status":"completed"}),
    );
    previous.entries.last_mut().unwrap().id = "tool".into();
    AcpThread::push(
        &mut previous,
        "steering",
        json!({"type":"text", "text":"Correction"}),
    );
    previous.entries.last_mut().unwrap().delivery = previous.entries[2].delivery.clone();
    AcpThread::push(
        &mut previous,
        "steering",
        json!({"type":"image", "data":"abc", "mimeType":"image/png"}),
    );
    previous.entries.last_mut().unwrap().delivery = previous.entries[2].delivery.clone();
    message(&mut previous, Some("followup"), "Done");
    previous.stop_reason = Some("end_turn".into());
    finish(&mut previous);
    let mut replay = snapshot();
    replay.entries = [0, 1, 5, 2, 6, 7, 8]
        .into_iter()
        .map(|index| previous.entries[index].clone())
        .collect();
    replay.entries[1].content["text"] = "First answer".into();
    for (index, entry) in replay.entries.iter_mut().enumerate() {
        if entry.kind != "tool" {
            entry.id = format!("replayed-{index}");
        }
        if entry.kind == "steering" {
            entry.kind = "user".into();
        }
        entry.delivery = None;
    }
    (previous, replay)
}

#[test]
fn replay_verifies_repeated_inputs_images_and_tools_and_keeps_unconsumed_cards() {
    let (previous, mut replay) = steering_replay();
    restore(&previous, &mut replay);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(replay.turns[0].status, "completed");
    assert_eq!(replay.turns[0].user_entry_id, "replayed-0");
    let cards: Vec<_> = replay
        .entries
        .iter()
        .filter(|entry| entry.kind == "steering")
        .collect();
    assert_eq!(cards.len(), 4);
    assert_eq!(
        cards
            .iter()
            .filter(|entry| entry.delivery.as_ref().unwrap().status == "injected")
            .count(),
        3
    );
    assert!(
        cards
            .iter()
            .any(|entry| entry.delivery.as_ref().unwrap().status == "failed")
    );
}

#[test]
fn replay_rejects_mismatched_output_identity_content_and_ambiguous_delivery() {
    for mismatch in [
        "identity",
        "content",
        "image",
        "tool_id",
        "tool_input",
        "tool_status",
        "ambiguous",
    ] {
        let (mut previous, mut replay) = steering_replay();
        match mismatch {
            "identity" => replay.entries[1].message_id = Some("unrelated".into()),
            "content" => replay.entries[1].content["text"] = "Different answer".into(),
            "image" => replay.entries[5].content["data"] = "different".into(),
            "tool_id" => replay.entries[2].id = "different-call".into(),
            "tool_input" => replay.entries[2].content["rawInput"] = json!({"command":"different"}),
            "tool_status" => replay.entries[2].content["status"] = "failed".into(),
            _ => previous.entries[2].delivery.as_mut().unwrap().status = "unknown".into(),
        }
        restore(&previous, &mut replay);
        assert!(replay.turns.is_empty(), "{mismatch}");
        assert!(
            replay.entries.iter().all(|entry| entry.kind != "steering"),
            "{mismatch}"
        );
    }
}

#[test]
fn replay_preserves_verified_prefix_when_native_history_has_later_prompts() {
    let (previous, mut replay) = steering_replay();
    AcpThread::push(
        &mut replay,
        "user",
        json!({"type":"text", "text":"Correction"}),
    );
    message(&mut replay, Some("later"), "New response");
    restore(&previous, &mut replay);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(replay.entries[replay.entries.len() - 2].kind, "user");
    assert_eq!(
        replay
            .entries
            .iter()
            .filter(|entry| entry.kind == "steering")
            .count(),
        4
    );
}

#[test]
fn replay_splits_combined_input_blocks_only_after_verifying_the_full_transcript() {
    let (previous, mut replay) = steering_replay();
    replay.entries[3].content["text"] = "CorrectionCorrection".into();
    replay.entries.remove(4);
    restore(&previous, &mut replay);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay
            .entries
            .iter()
            .filter(|entry| entry.kind == "steering")
            .count(),
        4
    );
    let ids: std::collections::BTreeSet<_> = replay.entries.iter().map(|entry| &entry.id).collect();
    assert_eq!(ids.len(), replay.entries.len());
}

#[test]
fn unconsumed_cards_do_not_overtake_earlier_submissions_when_responses_move() {
    let (mut previous, mut replay) = steering_replay();
    let failed = previous.entries.remove(3);
    previous.entries.insert(4, failed);
    restore(&previous, &mut replay);
    let inputs: Vec<_> = replay
        .entries
        .iter()
        .filter(|entry| entry.kind == "steering")
        .collect();
    assert_eq!(inputs[0].content["text"], "Correction");
    assert_eq!(inputs[1].content["text"], "Unsent");
}
