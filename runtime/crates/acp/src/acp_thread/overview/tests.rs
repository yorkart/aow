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
fn recorded_codex_steering_keeps_final_replies_visible_to_dialogue_clients() -> Result<()> {
    let recording: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-steering-recording.json"
    ))?;
    let mut projections = serde_json::Map::new();
    for capture in recording["captures"].as_array().unwrap() {
        if let Some(before) = capture.get("before_steer") {
            let mut thread: SessionSnapshot = serde_json::from_value(before.clone())?;
            let view = snapshot(&mut thread);
            let turn = &view.overview.as_ref().unwrap()[0];
            assert!(turn.running);
            assert!(turn.conclusion.is_empty());
            if let Some(expected_overview) = capture.get("before_steer_overview") {
                assert_eq!(serde_json::to_value(&view.overview)?, *expected_overview);
            }
            projections.insert(
                format!("{}-before-steer", capture["name"].as_str().unwrap()),
                serde_json::to_value(view.overview)?,
            );
        }
        let mut thread: SessionSnapshot = serde_json::from_value(capture["snapshot"].clone())?;
        let expected = thread.entries.last().unwrap().clone();
        assert_eq!(thread.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(expected.kind, "assistant");
        let view = snapshot(&mut thread);
        let turn = &view.overview.as_ref().unwrap()[0];
        assert_eq!(turn.conclusion.len(), 1);
        assert_eq!(turn.conclusion[0].content, expected.content);
        assert_eq!(turn.process_segments.len(), 2);
        // Already-open clients select messages in preference to conclusion.
        // A steering-only projection reproduces the missing answer screenshot.
        assert!(
            turn.messages
                .as_ref()
                .unwrap()
                .iter()
                .any(|entry| entry.id == expected.id),
            "{}: legacy dialogue selection must include the recorded final reply",
            capture["name"]
        );
        if let Some(expected_overview) = capture.get("overview") {
            assert_eq!(serde_json::to_value(&view.overview)?, *expected_overview);
        }
        projections.insert(
            capture["name"].as_str().unwrap().into(),
            serde_json::to_value(view.overview)?,
        );
    }
    if let Ok(path) = std::env::var("AOW_ACP_RECORD_PROJECTIONS") {
        std::fs::write(path, serde_json::to_vec_pretty(&projections)?)?;
    }
    Ok(())
}

#[test]
fn recorded_codex_load_keeps_steering_despite_tool_transport_changes() -> Result<()> {
    let recording: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-steering-replay.json"
    ))?;
    let previous: SessionSnapshot = serde_json::from_value(recording["live"].clone())?;
    let mut replay = previous.clone();
    replay.entries.clear();
    replay.turns.clear();
    for update in recording["updates"].as_array().unwrap() {
        AcpThread::handle_session_update(&mut replay, update.clone());
    }
    for (index, entry) in replay.entries.iter_mut().enumerate() {
        if entry.kind != "tool" {
            entry.id = format!("reloaded-{index}");
        }
    }
    assert_eq!(
        replay
            .entries
            .iter()
            .filter(|entry| entry.kind == "user")
            .count(),
        2
    );
    crate::acp_thread::turns::restore(&previous, &mut replay);
    assert_eq!(
        replay
            .entries
            .iter()
            .filter(|entry| entry.kind == "steering")
            .count(),
        1
    );
    assert_eq!(replay.turns.len(), 1);
    let view = snapshot(&mut replay);
    let turns = view.overview.as_ref().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].process_segments.len(), 2);
    assert_eq!(
        turns[0].conclusion.last().unwrap().content,
        previous.entries.last().unwrap().content
    );
    if let Some(expected) = recording.get("reloaded") {
        assert_eq!(serde_json::to_value(&view.overview)?, expected["overview"]);
    }
    if let Ok(path) = std::env::var("AOW_ACP_RECORD_REPLAY") {
        // Share the actual reducer/restoration result with the browser regression.
        replay.overview = view.overview;
        std::fs::write(path, serde_json::to_vec_pretty(&replay)?)?;
    }
    Ok(())
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
fn steering_segments_preserve_progress_through_streaming_completion_and_reload() -> Result<()> {
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
    assert_eq!(turn.messages.as_ref().unwrap().len(), 1);
    assert_eq!(turn.progress_count, 2, "agent progress stays folded");
    assert_eq!(turn.tool_count, 2);
    assert_eq!(turn.process_segments.len(), 2);
    assert_eq!(
        turn.process_segments[0].progress_count, 1,
        "split message stays in its original segment"
    );
    assert_eq!(turn.process_segments[1].progress_count, 1);
    let turn_id = turn.id.clone();
    store.session_update("local", json!({"sessionUpdate":"agent_message_chunk", "messageId":"constraints", "content":{"type":"text", "text":" documented."}}))?;
    let SessionUpdates::Overview { turns, .. } = since(
        &mut source.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    ) else {
        panic!("streaming overview update")
    };
    assert!(
        turns[0].messages.is_none(),
        "progress bodies are fetched only with expanded activity"
    );
    store.update("local", |thread| {
        thread.status = "idle".into();
        thread.stop_reason = Some("end_turn".into());
    })?;
    let SessionUpdates::Snapshot {
        snapshot: completed,
    } = since(
        &mut source.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    )
    else {
        panic!("completion resets the dialogue baseline");
    };
    let completed_turn = &completed.overview.as_ref().unwrap()[0];
    assert_eq!(completed_turn.messages.as_ref().unwrap().len(), 2);
    assert_eq!(completed_turn.conclusion.len(), 1);
    drop(source);
    drop(store);
    let mut restored = ThreadStore::new(Some(directory.path().into()), || {})?;
    let source = restored.thread("local")?;
    let mut thread = source.lock().unwrap();
    let view = snapshot(&mut thread);
    let turn = &view.overview.as_ref().unwrap()[0];
    assert_eq!(turn.status.as_deref(), Some("completed"));
    assert_eq!(turn.messages.as_ref().unwrap().len(), 2);
    assert_eq!(turn.progress_count, 1);
    assert_eq!(
        turn.process_segments[1].progress_count, 0,
        "final reply is excluded from activity"
    );
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
fn final_message_crosses_only_identified_steering_fragments() {
    let entry = |id: &str, kind: &str, message_id: Option<&str>| ThreadEntry {
        id: id.into(),
        kind: kind.into(),
        message_id: message_id.map(str::to_owned),
        content: json!({"type":"text", "text":id}),
        delivery: None,
    };
    let head = entry("**hello", "assistant", Some("reply"));
    let steer = entry("instruction", "steering", None);
    let tail = entry("world**", "assistant", Some("reply"));
    let entries = [head.clone(), steer.clone(), steer.clone(), tail.clone()];
    assert_eq!(
        final_entries(&[
            entry("    ", "assistant", Some("reply")),
            steer.clone(),
            tail.clone()
        ])
        .len(),
        2,
        "whitespace fragments retain the final message's first render ID"
    );
    assert_eq!(
        final_entries(&entries)
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["**hello", "world**"]
    );
    for boundary in [
        entry("tool", "tool", None),
        entry("thought", "thought", None),
        entry("different", "assistant", Some("other")),
    ] {
        assert_eq!(
            final_entries(&[head.clone(), boundary, steer.clone(), tail.clone()]).len(),
            1
        );
    }
    assert!(
        final_entries(&[head, steer]).is_empty(),
        "a steer after a reply is not completion"
    );
    assert_eq!(
        final_entries(&[
            entry("head", "assistant", None),
            entry("steer", "steering", None),
            entry("tail", "assistant", None)
        ])
        .len(),
        1
    );
}

#[test]
fn multiple_steers_keep_tool_updates_and_permissions_in_the_original_segment() -> Result<()> {
    let mut store = history()?;
    store.update("local", |thread| {
        thread.status = "working".into();
        thread.permissions.push(crate::facade::Permission {
            id: "permission".into(),
            kind: "permission".into(),
            request: json!({"toolCall":{"toolCallId":"tool-1"}}),
        });
        for (kind, value) in [
            ("steering", "First change"),
            ("assistant", "Working"),
            ("steering", "Second change"),
            ("assistant", "Continuing"),
        ] {
            AcpThread::push(thread, kind, json!({"type":"text", "text":value}));
        }
    })?;
    let source = store.thread("local")?;
    let before = snapshot(&mut source.lock().unwrap());
    let before_turn = before.overview.as_ref().unwrap().last().unwrap();
    assert_eq!(before_turn.process_segments.len(), 3);
    assert_eq!(
        before_turn.process_segments[0].permission_tool_ids,
        ["tool-1"]
    );
    assert!(
        before_turn.process_segments[1..]
            .iter()
            .all(|segment| segment.permission_tool_ids.is_empty())
    );
    assert!(
        before.overview.as_ref().unwrap()[0].process_segments[0]
            .permission_tool_ids
            .is_empty()
    );
    store.session_update(
        "local",
        json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool-1", "status":"completed"}),
    )?;
    let after = snapshot(&mut source.lock().unwrap());
    let after_turn = after.overview.as_ref().unwrap().last().unwrap();
    assert_eq!(
        after_turn
            .process_segments
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<Vec<_>>(),
        before_turn
            .process_segments
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        after_turn
            .process_segments
            .iter()
            .map(|segment| segment.tool_count)
            .collect::<Vec<_>>(),
        [1, 0, 0]
    );
    assert_eq!(
        after_turn
            .process_segments
            .iter()
            .map(|segment| segment.progress_count)
            .collect::<Vec<_>>(),
        [1, 1, 1]
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
fn steering_overviews_exclude_progress_bodies_and_reset_invalid_projection_cursors() -> Result<()> {
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
    assert!(delta["turns"][0].get("messages").is_none());
    assert_eq!(
        delta["turns"][0]["process_segments"][1]["progress_count"],
        1
    );
    let bytes = serde_json::to_vec(&delta)?.len();
    eprintln!("steered overview delta: {bytes} bytes for a 1 MiB transcript");
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
