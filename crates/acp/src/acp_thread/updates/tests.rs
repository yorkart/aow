use super::*;
use crate::acp_thread::{AcpThread, history::ThreadStore};
use anyhow::Result;
use serde_json::json;

fn store() -> Result<ThreadStore> {
    let mut store = ThreadStore::new(None, || {})?;
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "test".into(),
        "/repo".into(),
    );
    for _ in 0..100 {
        AcpThread::push(
            &mut thread,
            "assistant",
            json!({"type":"text", "text":"x".repeat(10_000)}),
        );
    }
    store.insert(thread)?;
    store.update("local", |thread| thread.status = "working".into())?;
    Ok(store)
}

#[test]
fn text_deltas_are_bounded_by_new_content_and_preserve_utf8() -> Result<()> {
    let mut store = store()?;
    let before = store.snapshot("local")?;
    for text in ["你好", "🌍"] {
        store.session_update(
            "local",
            json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text", "text":text}}),
        )?;
    }
    let thread = store.thread("local")?;
    let current = thread.lock().unwrap();
    let delta = since(&current, &before.sync_generation, before.revision);
    let json = serde_json::to_value(&delta)?;
    assert_eq!(json["kind"], "delta");
    assert_eq!(json["entries"][0]["kind"], "append_text");
    assert_eq!(json["entries"][0]["text"], "你好🌍");
    assert!(
        serde_json::to_vec(&delta)?.len() < 1000,
        "history must not enter a streamed response"
    );
    assert!(matches!(
        since(&current, &current.sync_generation, current.revision),
        SessionUpdates::Unchanged
    ));
    Ok(())
}

#[test]
fn terminal_deltas_append_once_and_do_not_retain_output_in_tools() -> Result<()> {
    let mut store = store()?;
    store.session_update("local", json!({"sessionUpdate":"tool_call", "toolCallId":"tool", "_meta":{"terminal_info":{"terminal_id":"term"}}}))?;
    store.session_update("local", json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool", "_meta":{"terminal_output":{"terminal_id":"term", "data":"first\n"}}}))?;
    let before = store.snapshot("local")?;
    store.session_update("local", json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool", "_meta":{"terminal_output":{"terminal_id":"term", "data":"second\n"}, "other":true}}))?;
    let thread = store.thread("local")?;
    let current = thread.lock().unwrap();
    let delta = serde_json::to_value(since(&current, &before.sync_generation, before.revision))?;
    assert_eq!(
        delta["terminals"][0],
        json!({"kind":"append_output", "id":"term", "text":"second\n"})
    );
    assert_eq!(current.terminals["term"]["output"], "first\nsecond\n");
    assert!(
        current.entries.last().unwrap().content["_meta"]
            .get("terminal_output")
            .is_none()
    );
    assert_eq!(
        current.entries.last().unwrap().content["_meta"]["other"],
        true
    );
    Ok(())
}

#[test]
fn gaps_resets_and_new_incarnations_resynchronize() -> Result<()> {
    let mut store = store()?;
    let before = store.snapshot("local")?;
    for _ in 0..=HISTORY_LIMIT {
        store.session_update("local", json!({"sessionUpdate":"usage_update", "used":1}))?;
    }
    let thread = store.thread("local")?;
    {
        let current = thread.lock().unwrap();
        assert!(matches!(
            since(&current, &before.sync_generation, before.revision),
            SessionUpdates::Snapshot { .. }
        ));
        assert!(matches!(
            since(&current, "old-server", current.revision),
            SessionUpdates::Snapshot { .. }
        ));
        assert!(matches!(
            since(&current, &current.sync_generation, current.revision + 1),
            SessionUpdates::Snapshot { .. }
        ));
    }
    let before = store.snapshot("local")?;
    store.update("local", |thread| {
        thread.entries.clear();
        thread.status = "idle".into();
    })?;
    assert!(matches!(
        since(
            &thread.lock().unwrap(),
            &before.sync_generation,
            before.revision
        ),
        SessionUpdates::Snapshot { .. }
    ));
    Ok(())
}

#[test]
fn replacement_wins_over_appends_in_the_same_interval() -> Result<()> {
    let mut store = store()?;
    let before = store.snapshot("local")?;
    for text in ["new", " message"] {
        store.session_update(
            "local",
            json!({"sessionUpdate":"agent_thought_chunk", "content":{"type":"text", "text":text}}),
        )?;
    }
    store.session_update("local", json!({"sessionUpdate":"plan", "entries":[]}))?;
    let thread = store.thread("local")?;
    let delta = serde_json::to_value(since(
        &thread.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    ))?;
    assert_eq!(delta["entries"][0]["kind"], "replace");
    assert_eq!(
        delta["entries"][0]["entry"]["content"]["text"],
        "new message"
    );
    assert_eq!(delta["fields"]["plan"]["entries"], json!([]));
    Ok(())
}

#[test]
fn mixed_protocol_deltas_reconstruct_the_authoritative_snapshot() -> Result<()> {
    let mut store = store()?;
    let before = store.snapshot("local")?;
    for event in [
        json!({"sessionUpdate":"agent_message_chunk", "messageId":"message", "content":{"type":"text", "text":"first"}}),
        json!({"sessionUpdate":"agent_message_chunk", "messageId":"message", "content":{"type":"text", "text":" 你好"}}),
        json!({"sessionUpdate":"tool_call", "toolCallId":"tool", "title":"command", "_meta":{"terminal_info":{"terminal_id":"term"}}}),
        json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool", "status":"in_progress", "_meta":{"terminal_output":{"terminal_id":"term", "data":"output"}}}),
        json!({"sessionUpdate":"compaction_update", "compactionId":"c", "status":"in_progress"}),
        json!({"sessionUpdate":"compaction_summary_chunk", "compactionId":"c", "content":{"type":"text", "text":"summary"}}),
        json!({"sessionUpdate":"plan", "entries":[]}),
        json!({"sessionUpdate":"notice", "title":"notice"}),
        json!({"sessionUpdate":"current_mode_update", "currentModeId":"mode"}),
        json!({"sessionUpdate":"config_option_update", "configOptions":[]}),
        json!({"sessionUpdate":"available_commands_update", "availableCommands":[]}),
        json!({"sessionUpdate":"session_info_update", "title":"new title", "_meta":{"trae":{"queueStatus":{"state":"waiting","message":"Queue position: 2"}}}}),
        json!({"sessionUpdate":"usage_update", "used":42}),
        json!({"sessionUpdate":"future_extension", "value":true}),
    ] {
        store.session_update("local", event)?;
    }
    let thread = store.thread("local")?;
    let current = thread.lock().unwrap();
    let delta = serde_json::to_value(since(&current, &before.sync_generation, before.revision))?;
    let mut reconstructed = serde_json::to_value(before)?;
    for patch in delta["entries"].as_array().unwrap() {
        let index = patch["index"].as_u64().unwrap() as usize;
        let entries = reconstructed["entries"].as_array_mut().unwrap();
        if patch["kind"] == "replace" {
            if index == entries.len() {
                entries.push(patch["entry"].clone());
            } else {
                entries[index] = patch["entry"].clone();
            }
        } else {
            let Value::String(text) = &mut entries[index]["content"]["text"] else {
                panic!("text append target")
            };
            text.push_str(patch["text"].as_str().unwrap());
        }
    }
    for patch in delta["terminals"].as_array().unwrap() {
        let id = patch["id"].as_str().unwrap();
        assert_eq!(patch["kind"], "replace");
        reconstructed["terminals"][id] = patch["terminal"].clone();
    }
    for (key, value) in delta["fields"].as_object().unwrap() {
        reconstructed[key] = value.clone();
    }
    for key in ["revision", "updated_at"] {
        reconstructed[key] = delta[key].clone();
    }
    assert_eq!(reconstructed, serde_json::to_value(&*current)?);
    Ok(())
}

#[test]
#[ignore = "manual synthetic throughput sample; no timing assertions"]
fn streaming_hot_path_sample() -> Result<()> {
    for history_mib in [1, 10] {
        let mut store = ThreadStore::new(None, || {})?;
        let mut thread = AcpThread::new_snapshot(
            "local".into(),
            "remote".into(),
            "test".into(),
            "/repo".into(),
        );
        for _ in 0..1000 {
            AcpThread::push(
                &mut thread,
                "assistant",
                json!({"type":"text", "text":"x".repeat(history_mib * 1024 * 1024 / 1000)}),
            );
        }
        store.insert(thread)?;
        store.update("local", |thread| thread.status = "working".into())?;
        let before = store.snapshot("local")?;
        let snapshot_bytes = serde_json::to_vec(&before)?.len();
        let mut durations = Vec::new();
        for _ in 0..500 {
            let event = json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text", "text":"next chunk"}});
            let start = std::time::Instant::now();
            store.session_update("local", event)?;
            durations.push(start.elapsed().as_nanos());
        }
        durations.sort_unstable();
        let thread = store.thread("local")?;
        let current = thread.lock().unwrap();
        let delta_bytes =
            serde_json::to_vec(&since(&current, &before.sync_generation, before.revision))?.len();
        println!(
            "{}",
            json!({"history_mib":history_mib, "updates":500, "snapshot_bytes":snapshot_bytes, "delta_bytes":delta_bytes,
            "update_p50_ns":durations[250], "update_p95_ns":durations[475]})
        );
    }
    Ok(())
}

#[test]
fn session_metadata_replaces_top_level_keys_and_keeps_ready_payloads() -> Result<()> {
    let mut store = store()?;
    let before = store.snapshot("local")?;
    let history_size = before.entries.len();
    for metadata in [
        json!({"trae":{"queueStatus":{"state":"waiting","message":"Queue position: 2"},"old":true},"other":{"value":1}}),
        json!({"trae":{"queueStatus":{"state":"ready","position":null,"message":null}}}),
    ] {
        store.session_update(
            "local",
            json!({"sessionUpdate":"session_info_update","_meta":metadata}),
        )?;
    }
    store.session_update(
        "local",
        json!({"sessionUpdate":"session_info_update","title":"Updated"}),
    )?;
    let current = store.snapshot("local")?;
    assert_eq!(current.entries.len(), history_size);
    assert_eq!(current.session_meta_revision, 2);
    assert_eq!(current.session_meta_revisions["trae"], 2);
    assert_eq!(current.session_meta_revisions["other"], 1);
    assert_eq!(current.session_meta["other"], json!({"value":1}));
    assert_eq!(
        current.session_meta["trae"],
        json!({"queueStatus":{"state":"ready","position":null,"message":null}})
    );
    let thread = store.thread("local")?;
    let delta = serde_json::to_value(since(
        &thread.lock().unwrap(),
        &before.sync_generation,
        before.revision,
    ))?;
    assert_eq!(
        delta["fields"]["session_meta"],
        serde_json::to_value(&current.session_meta)?
    );
    let overview = crate::acp_thread::overview::snapshot(&mut thread.lock().unwrap());
    assert_eq!(overview.session_meta, current.session_meta);
    assert_eq!(overview.session_meta_revisions, current.session_meta_revisions);
    store.session_update(
        "local",
        json!({"sessionUpdate":"session_info_update","_meta":null}),
    )?;
    assert!(store.snapshot("local")?.session_meta.is_empty());
    assert!(store.snapshot("local")?.session_meta_revisions.is_empty());
    Ok(())
}
