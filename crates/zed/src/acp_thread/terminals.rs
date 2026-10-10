//! Zed's legacy display-terminal metadata, without terminal execution or a PTY.
use crate::facade::SessionSnapshot;
use serde_json::{Value, json};

pub(super) fn apply_legacy_update(thread: &mut SessionSnapshot, update: &Value) {
    let meta = &update["_meta"];
    match update["sessionUpdate"].as_str() {
        Some("tool_call") => {
            let info = &meta["terminal_info"];
            if let Some(id) = info["terminal_id"].as_str() {
                let terminal = terminal(thread, id);
                if let Some(command) = update["title"].as_str() {
                    terminal["command"] = command.into();
                }
                if let Some(cwd) = info["cwd"].as_str() {
                    terminal["cwd"] = cwd.into();
                }
            }
        }
        Some("tool_call_update") => {
            let output = &meta["terminal_output"];
            if let (Some(id), Some(data)) =
                (output["terminal_id"].as_str(), output["data"].as_str())
            {
                let terminal = terminal(thread, id);
                if let Some(Value::String(output)) = terminal.get_mut("output") {
                    output.push_str(data);
                }
            }
            let exit = &meta["terminal_exit"];
            if let Some(id) = exit["terminal_id"].as_str() {
                terminal(thread, id)["exit_status"] = json!({
                    "exitCode": exit["exit_code"],
                    "signal": exit["signal"],
                });
            }
        }
        _ => {}
    }
}

fn terminal<'a>(thread: &'a mut SessionSnapshot, id: &str) -> &'a mut Value {
    thread
        .terminals
        .entry(id.into())
        .or_insert_with(|| json!({"output": ""}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_thread::AcpThread;

    #[test]
    fn terminal_chunks_survive_tool_patches_and_history_round_trip() {
        let mut thread = AcpThread::new_snapshot(
            "local".into(),
            "remote".into(),
            "agent".into(),
            "/repo".into(),
        );
        // Output and exit may arrive before the tool's terminal metadata.
        for update in [
            json!({"sessionUpdate":"tool_call_update","toolCallId":"tool", "_meta":{"terminal_output":{"terminal_id":"term","data":"early\n"}}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"pwd","kind":"execute","content":[{"type":"terminal","terminalId":"term"}],"_meta":{"terminal_info":{"terminal_id":"term","cwd":"/repo"}}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"tool", "_meta":{"terminal_output":{"terminal_id":"term","data":"/repo\n"}}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"tool", "_meta":{"terminal_exit":{"terminal_id":"term","exit_code":0}}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"completed","_meta":{"unrelated":true}}),
        ] {
            AcpThread::handle_session_update(&mut thread, update);
        }
        assert_eq!(thread.entries.len(), 1);
        assert_eq!(thread.entries[0].content["status"], "completed");
        assert_eq!(
            thread.entries[0].content["content"][0]["terminalId"],
            "term"
        );
        assert_eq!(thread.terminals["term"]["output"], "early\n/repo\n");
        assert_eq!(thread.terminals["term"]["cwd"], "/repo");
        assert_eq!(thread.terminals["term"]["exit_status"]["exitCode"], 0);
        let saved = serde_json::to_value(&thread).unwrap();
        let restored: SessionSnapshot = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(restored.terminals, thread.terminals);
        let mut old_history = saved;
        old_history.as_object_mut().unwrap().remove("terminals");
        let restored: SessionSnapshot = serde_json::from_value(old_history).unwrap();
        assert!(restored.terminals.is_empty());
    }

    #[test]
    fn terminals_are_isolated_and_metadata_does_not_reset_output_or_exit() {
        let mut thread = AcpThread::new_snapshot(
            "local".into(),
            "remote".into(),
            "agent".into(),
            "/repo".into(),
        );
        for (id, data, code) in [("one", "first", 0), ("two", "second", 1)] {
            apply_legacy_update(
                &mut thread,
                &json!({"sessionUpdate":"tool_call_update","_meta":{
                    "terminal_output":{"terminal_id":id,"data":data},
                    "terminal_exit":{"terminal_id":id,"exit_code":code,"signal":"SIGTERM"}
                }}),
            );
            apply_legacy_update(
                &mut thread,
                &json!({"sessionUpdate":"tool_call","title":"command","_meta":{"terminal_info":{"terminal_id":id}}}),
            );
            assert_eq!(thread.terminals[id]["output"], data);
            assert_eq!(thread.terminals[id]["exit_status"]["exitCode"], code);
            assert_eq!(thread.terminals[id]["exit_status"]["signal"], "SIGTERM");
        }
        apply_legacy_update(
            &mut thread,
            &json!({"sessionUpdate":"tool_call_update","_meta":{"terminal_output":{"data":"missing id"}}}),
        );
        assert_eq!(thread.terminals.len(), 2);
    }
}
