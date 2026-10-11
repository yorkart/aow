use super::*;
use crate::acp_thread::history::ThreadStore;
use serde_json::json;

fn history() -> Result<ThreadStore> {
    let mut store = ThreadStore::new(None)?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    for index in 0..2 {
        AcpThread::push(
            &mut thread,
            "user",
            json!({"type":"text", "text":format!("question {index}")}),
        );
        AcpThread::handle_session_update(
            &mut thread,
            json!({"sessionUpdate":"tool_call", "toolCallId":format!("tool-{index}"), "title":"command", "content":[{"type":"terminal", "terminalId":format!("term-{index}") }]}),
        );
        thread.terminals.insert(
            format!("term-{index}"),
            json!({"output":"x".repeat(5 * 1024 * 1024)}),
        );
        AcpThread::push(
            &mut thread,
            "assistant",
            json!({"type":"text", "text":format!("answer {index}")}),
        );
    }
    store.insert(thread)?;
    Ok(store)
}

#[test]
fn collapsed_overview_excludes_bodies_and_details_are_scoped() -> Result<()> {
    let mut store = history()?;
    let source = store.thread("local")?;
    let mut thread = source.lock().unwrap();
    let overview = snapshot(&mut thread);
    assert!(overview.entries.is_empty());
    assert!(overview.terminals.is_empty());
    assert!(serde_json::to_vec(&overview)?.len() < 5000);
    let turns = overview.overview.unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].tool_count, 1);
    assert_eq!(turns[0].conclusion[0].content["text"], "answer 0");
    let details = process(&mut thread, &turns[0].id, None, None)?;
    let SessionUpdates::Snapshot { snapshot } = details else {
        panic!("initial details")
    };
    assert_eq!(
        snapshot.entries.len(),
        1,
        "details do not repeat the prompt or conclusion"
    );
    assert_eq!(snapshot.terminals.len(), 1);
    assert!(snapshot.terminals.contains_key("term-0"));
    assert!(process(&mut thread, "missing", None, None).is_err());
    Ok(())
}

#[test]
fn overview_and_process_updates_do_not_resend_other_turns() -> Result<()> {
    let mut store = history()?;
    store.update("local", |thread| thread.status = "working".into())?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    let turn_id = before.overview.as_ref().unwrap()[1].id.clone();
    let SessionUpdates::Snapshot { snapshot: details } =
        process(&mut source.lock().unwrap(), &turn_id, None, None)?
    else {
        panic!()
    };
    store.session_update(
        "local",
        json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":" suffix"}}),
    )?;
    let mut thread = source.lock().unwrap();
    let SessionUpdates::Overview { turns, .. } =
        since(&mut thread, &before.sync_generation, before.revision)
    else {
        panic!()
    };
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].id, turn_id);
    assert!(
        turns[0].user.is_none(),
        "unchanged user text/images must not be repeated"
    );
    assert!(
        turns[0].conclusion.is_empty(),
        "streaming progress is not a conclusion"
    );
    let delta = serde_json::to_value(process(
        &mut thread,
        &turn_id,
        Some(&details.sync_generation),
        Some(details.revision),
    )?)?;
    assert_eq!(delta["entries"][0]["index"], 1);
    assert_eq!(delta["entries"][0]["text"], " suffix");
    assert_eq!(delta["terminals"], json!([]));
    assert!(serde_json::to_vec(&delta)?.len() < 1000);
    Ok(())
}

#[test]
fn finish_and_reload_reset_overviews_without_losing_process() -> Result<()> {
    let mut store = history()?;
    store.update("local", |thread| thread.status = "working".into())?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    store.update("local", |thread| {
        thread.status = "idle".into();
        thread.stop_reason = Some("end_turn".into());
    })?;
    let mut thread = source.lock().unwrap();
    let SessionUpdates::Snapshot { snapshot } =
        since(&mut thread, &before.sync_generation, before.revision)
    else {
        panic!()
    };
    let turn = &snapshot.overview.as_ref().unwrap()[1];
    assert!(!turn.running);
    assert_eq!(turn.conclusion[0].content["text"], "answer 1");
    assert_eq!(turn.tool_count, 1);
    assert!(matches!(
        since(&mut thread, "previous-server", snapshot.revision),
        SessionUpdates::Snapshot { .. }
    ));
    assert!(matches!(
        process(
            &mut thread,
            &turn.id,
            Some("previous-server"),
            Some(snapshot.revision)
        )?,
        SessionUpdates::Snapshot { .. }
    ));
    Ok(())
}

#[test]
fn newly_referenced_terminal_is_sent_even_when_output_arrived_earlier() -> Result<()> {
    let mut store = history()?;
    store.update("local", |thread| thread.status = "working".into())?;
    let source = store.thread("local")?;
    let overview = snapshot(&mut source.lock().unwrap());
    let turn_id = &overview.overview.as_ref().unwrap()[1].id;
    store.session_update("local", json!({"sessionUpdate":"tool_call_update", "toolCallId":"new-tool", "_meta":{"terminal_output":{"terminal_id":"early", "data":"already here"}}}))?;
    let SessionUpdates::Snapshot { snapshot: before } =
        process(&mut source.lock().unwrap(), turn_id, None, None)?
    else {
        panic!()
    };
    assert!(!before.terminals.contains_key("early"));
    store.session_update("local", json!({"sessionUpdate":"tool_call", "toolCallId":"new-tool", "content":[{"type":"terminal", "terminalId":"early"}]}))?;
    let delta = serde_json::to_value(process(
        &mut source.lock().unwrap(),
        turn_id,
        Some(&before.sync_generation),
        Some(before.revision),
    )?)?;
    assert_eq!(delta["terminals"][0]["kind"], "replace");
    assert_eq!(delta["terminals"][0]["terminal"]["output"], "already here");
    Ok(())
}

#[test]
fn failed_and_tool_only_turns_never_promote_progress_to_a_conclusion() -> Result<()> {
    let mut store = history()?;
    store.update("local", |thread| {
        thread.status = "idle".into();
        thread.stop_reason = Some("cancelled".into());
        AcpThread::push(
            thread,
            "tool",
            json!({"title":"last tool", "status":"completed"}),
        );
    })?;
    let source = store.thread("local")?;
    let view = snapshot(&mut source.lock().unwrap());
    let last = view.overview.unwrap().pop().unwrap();
    assert_eq!(last.status.as_deref(), Some("interrupted"));
    assert!(last.conclusion.is_empty());
    assert_eq!(last.tool_count, 2);
    assert_eq!(last.progress_count, 1);
    Ok(())
}

#[test]
fn tool_visibility_changes_resynchronize_overviews_and_detail_positions() -> Result<()> {
    let mut store = ThreadStore::new(None)?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    thread.status = "working".into();
    AcpThread::push(
        &mut thread,
        "user",
        json!({"type":"text", "text":"first question"}),
    );
    let first = thread.entries[0].id.clone();
    AcpThread::handle_session_update(
        &mut thread,
        json!({"sessionUpdate":"tool_call", "toolCallId":"tool", "status":"cancelled"}),
    );
    AcpThread::push(
        &mut thread,
        "user",
        json!({"type":"text", "text":"second question"}),
    );
    AcpThread::push(
        &mut thread,
        "assistant",
        json!({"type":"text", "text":"reply"}),
    );
    store.insert(thread)?;
    let source = store.thread("local")?;
    let mut before = snapshot(&mut source.lock().unwrap());
    assert_eq!(before.overview.as_ref().unwrap().len(), 1);
    let SessionUpdates::Snapshot {
        snapshot: mut details,
    } = process(&mut source.lock().unwrap(), &first, None, None)?
    else {
        panic!()
    };
    assert_eq!(details.entries[0].content["text"], "reply");

    // A late output reveals the cancelled tool, splitting the imported user
    // blocks. Removing that output merges them again; old cursors must not
    // retain the removed turn or apply patches at the old process positions.
    for shown in [true, false, true] {
        let content = if shown {
            json!([{"type":"content", "content":{"type":"text", "text":"late output"}}])
        } else {
            json!([])
        };
        store.session_update(
            "local",
            json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool", "content":content}),
        )?;
        let mut thread = source.lock().unwrap();
        // Another browser can refresh the shared cache before this cursor reads.
        let other_view = snapshot(&mut thread);
        let fresh = snapshot(&mut thread.clone());
        assert_eq!(
            serde_json::to_value(&other_view.overview)?,
            serde_json::to_value(fresh.overview)?
        );
        let SessionUpdates::Snapshot { snapshot: updated } =
            since(&mut thread, &before.sync_generation, before.revision)
        else {
            panic!("boundary changes must replace the overview")
        };
        let turns = updated.overview.as_ref().unwrap();
        assert_eq!(turns.len(), if shown { 2 } else { 1 });
        assert_eq!(turns[0].user.len(), if shown { 1 } else { 2 });
        let SessionUpdates::Snapshot {
            snapshot: next_details,
        } = process(
            &mut thread,
            &first,
            Some(&details.sync_generation),
            Some(details.revision),
        )?
        else {
            panic!("boundary changes must replace process positions")
        };
        assert_eq!(next_details.entries.len(), 1);
        if shown {
            assert_eq!(next_details.entries[0].id, "tool");
        } else {
            assert_eq!(next_details.entries[0].content["text"], "reply");
        }
        before = *updated;
        details = next_details;
    }
    Ok(())
}

#[test]
fn newly_visible_leading_tools_rebuild_ranges_outside_the_old_index() -> Result<()> {
    let mut store = ThreadStore::new(None)?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    thread.status = "working".into();
    AcpThread::handle_session_update(
        &mut thread,
        json!({"sessionUpdate":"tool_call", "toolCallId":"leading-tool", "status":"cancelled"}),
    );
    AcpThread::push(
        &mut thread,
        "user",
        json!({"type":"text", "text":"question"}),
    );
    store.insert(thread)?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    store.session_update("local", json!({"sessionUpdate":"tool_call_update", "toolCallId":"leading-tool", "status":"completed"}))?;
    let mut thread = source.lock().unwrap();
    let SessionUpdates::Snapshot { snapshot: updated } =
        since(&mut thread, &before.sync_generation, before.revision)
    else {
        panic!()
    };
    let turns = updated.overview.unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "leading");
    assert_eq!(turns[0].tool_count, 1);
    let SessionUpdates::Snapshot { snapshot: details } =
        process(&mut thread, "leading", None, None)?
    else {
        panic!()
    };
    assert_eq!(details.entries[0].id, "leading-tool");
    Ok(())
}

#[test]
fn visibility_changes_with_stable_boundaries_keep_incremental_updates() -> Result<()> {
    let mut store = ThreadStore::new(None)?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    thread.status = "working".into();
    AcpThread::push(
        &mut thread,
        "user",
        json!({"type":"text", "text":"question"}),
    );
    AcpThread::handle_session_update(
        &mut thread,
        json!({"sessionUpdate":"tool_call", "toolCallId":"tool", "status":"in_progress"}),
    );
    store.insert(thread)?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    let turn_id = &before.overview.as_ref().unwrap()[0].id;
    store.session_update(
        "local",
        json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool", "status":"cancelled"}),
    )?;
    let mut thread = source.lock().unwrap();
    let SessionUpdates::Overview { turns, .. } =
        since(&mut thread, &before.sync_generation, before.revision)
    else {
        panic!("visibility alone need not reset a stable layout")
    };
    assert_eq!(turns[0].tool_count, 0);
    assert!(turns[0].user.is_none());
    assert!(matches!(
        process(
            &mut thread,
            turn_id,
            Some(&before.sync_generation),
            Some(before.revision)
        )?,
        SessionUpdates::Delta { .. }
    ));
    Ok(())
}
