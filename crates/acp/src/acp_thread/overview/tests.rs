use super::*;
use crate::acp_thread::history::ThreadStore;
use serde_json::json;

fn history() -> Result<ThreadStore> {
    let mut store = ThreadStore::new(None, || {})?;
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
fn browser_projections_preserve_creation_metadata() -> Result<()> {
    let mut store = history()?;
    let parent = Some(aow_protocol::InstanceRef::terminal("parent"));
    let created_at = "2026-01-01T00:00:00Z";
    store.update("local", |thread| {
        thread.parent = parent.clone();
        thread.source = crate::facade::SessionSource::Cli;
        thread.created_at = created_at.into();
        thread.status = "working".into();
    })?;
    let assert_metadata = |snapshot: &SessionSnapshot| {
        assert_eq!(snapshot.parent, parent);
        assert_eq!(snapshot.source, crate::facade::SessionSource::Cli);
        assert_eq!(snapshot.created_at, created_at);
    };
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    assert_metadata(&before);
    let turn_id = &before.overview.as_ref().unwrap()[1].id;
    let SessionUpdates::Snapshot { snapshot: details } =
        process(&mut source.lock().unwrap(), turn_id, None, None)?
    else {
        panic!("initial process snapshot")
    };
    assert_metadata(&details);
    store.session_update(
        "local",
        json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":" suffix"}}),
    )?;
    let mut thread = source.lock().unwrap();
    let SessionUpdates::Overview {
        snapshot: update, ..
    } = since(&mut thread, &before.sync_generation, before.revision)
    else {
        panic!("incremental overview")
    };
    assert_metadata(&update);
    let SessionUpdates::Snapshot { snapshot: reset } =
        since(&mut thread, "previous-server", before.revision)
    else {
        panic!("overview resynchronization")
    };
    assert_metadata(&reset);
    Ok(())
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
fn steered_dialogue_survives_streaming_completion_and_reload_without_hiding_tools() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = ThreadStore::new(Some(directory.path().into()), || {})?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    thread.status = "working".into();
    for (kind, id, value) in [
        ("user", None, "Explain the project"),
        ("tool", None, "First inspection"),
        ("assistant", Some("readme"), "Read **the "),
        ("steering", None, "Also explain AGENTS.md"),
        ("assistant", Some("readme"), "README**."),
        ("tool", None, "Follow-up inspection"),
        ("assistant", Some("constraints"), "The constraints are"),
    ] {
        AcpThread::push(&mut thread, kind, json!({"type":"text", "text":value}));
        thread.entries.last_mut().unwrap().message_id = id.map(str::to_owned);
    }
    crate::acp_thread::turns::start(&mut thread, 0);
    store.insert(thread)?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    let turn = &before.overview.as_ref().unwrap()[0];
    assert!(turn.conclusion.is_empty(), "streaming is not completion");
    assert_eq!(turn.messages.as_ref().unwrap().len(), 4);
    assert_eq!(turn.progress_count, 0, "dialogue is not folded progress");
    assert_eq!(turn.tool_count, 2);
    let turn_id = turn.id.clone();
    store.session_update("local", json!({"sessionUpdate":"agent_message_chunk", "messageId":"constraints", "content":{"type":"text", "text":" documented."}}))?;
    let SessionUpdates::Overview { turns, .. } = since(
        &mut source.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    ) else {
        panic!("streaming overview update")
    };
    assert_eq!(
        serde_json::to_value(&turns[0].messages)?["entries"][0]["text"],
        " documented."
    );
    store.update("local", |thread| {
        thread.status = "idle".into();
        thread.stop_reason = Some("end_turn".into());
    })?;
    drop(source);
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
    let source = restored.thread("local")?;
    let mut thread = source.lock().unwrap();
    let view = snapshot(&mut thread);
    let turn = &view.overview.as_ref().unwrap()[0];
    assert_eq!(turn.status.as_deref(), Some("completed"));
    assert_eq!(turn.messages.as_ref().unwrap().len(), 4);
    assert_eq!(
        turn.conclusion.len(),
        1,
        "final-answer inference stays conservative"
    );
    assert_eq!(
        turn.conclusion[0].message_id.as_deref(),
        Some("constraints")
    );
    let SessionUpdates::Snapshot { snapshot: details } =
        process(&mut thread, &turn_id, None, None)?
    else {
        panic!("process snapshot")
    };
    assert_eq!(
        details
            .entries
            .iter()
            .filter(|entry| entry.kind == "tool")
            .count(),
        2
    );
    assert_eq!(
        thread.entries[2].content["text"], "Read **the ",
        "saved arrival order is unchanged"
    );
    Ok(())
}

#[test]
fn tool_visibility_changes_resynchronize_overviews_and_detail_positions() -> Result<()> {
    let mut store = ThreadStore::new(None, || {})?;
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
    let mut store = ThreadStore::new(None, || {})?;
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
    let mut store = ThreadStore::new(None, || {})?;
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

#[test]
fn steering_streams_only_changed_dialogue_and_resets_invalid_projection_cursors() -> Result<()> {
    let mut store = ThreadStore::new(None, || {})?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    thread.status = "working".into();
    for (kind, content) in [
        ("user", json!({"type":"text", "text":"Task"})),
        (
            "steering",
            json!({"type":"image", "data":"i".repeat(512 * 1024), "mimeType":"image/png"}),
        ),
        (
            "assistant",
            json!({"type":"text", "text":"x".repeat(512 * 1024)}),
        ),
    ] {
        AcpThread::push(&mut thread, kind, content);
    }
    thread.entries[2].message_id = Some("answer".into());
    store.insert(thread)?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    for text in ["中", "z"] {
        store.session_update("local", json!({"sessionUpdate":"agent_message_chunk", "messageId":"answer", "content":{"type":"text", "text":text}}))?;
    }
    // Another viewer refreshing the shared cache must not consume our cursor.
    let latest = snapshot(&mut source.lock().unwrap());
    let delta = serde_json::to_value(since(
        &mut source.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    ))?;
    assert_eq!(delta["kind"], "overview");
    assert_eq!(delta["turns"][0]["messages"]["kind"], "delta");
    assert_eq!(
        delta["turns"][0]["messages"]["entries"][0]["kind"],
        "append_text"
    );
    assert_eq!(delta["turns"][0]["messages"]["entries"][0]["index"], 1);
    assert_eq!(delta["turns"][0]["messages"]["entries"][0]["text"], "中z");
    let bytes = serde_json::to_vec(&delta)?.len();
    eprintln!("steered dialogue delta: {bytes} bytes for a 1 MiB baseline");
    assert!(bytes < 8192);
    assert!(
        source.lock().unwrap().overview_cache.turns[0]
            .summary
            .messages
            .is_none(),
        "cache must not clone dialogue bodies"
    );
    store.session_update(
        "local",
        json!({"sessionUpdate":"tool_call", "toolCallId":"tool", "title":"Inspect"}),
    )?;
    let tool_update = serde_json::to_value(since(
        &mut source.lock().unwrap(),
        &latest.sync_generation,
        latest.revision,
    ))?;
    assert!(tool_update["turns"][0].get("messages").is_none());
    assert!(serde_json::to_vec(&tool_update)?.len() < 8192);
    let empty_base = snapshot(&mut source.lock().unwrap());
    store.session_update("local", json!({"sessionUpdate":"agent_message_chunk", "messageId":"new", "content":{"type":"text", "text":" "}}))?;
    let hidden = snapshot(&mut source.lock().unwrap());
    store.session_update("local", json!({"sessionUpdate":"agent_message_chunk", "messageId":"new", "content":{"type":"text", "text":"Visible"}}))?;
    for base in [&empty_base, &hidden] {
        assert!(matches!(
            since(
                &mut source.lock().unwrap(),
                &base.sync_generation,
                base.revision
            ),
            SessionUpdates::Snapshot { .. }
        ));
    }
    assert!(matches!(
        since(&mut source.lock().unwrap(), "stale", before.revision),
        SessionUpdates::Snapshot { .. }
    ));
    Ok(())
}
