use super::claude::claude_turns;
use super::*;
use std::{fs, path::PathBuf};

fn write(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fs::write(&path, contents).unwrap();
    (directory, path)
}

fn parse_records(records: Vec<Value>) -> Vec<SnapshotTurn> {
    let (_directory, path) = write(
        &records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    parse_codex_like(&path).unwrap()
}

#[test]
fn command_categories_and_input_survive_completion_projections() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"inspect code"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"exec-1","name":"exec_command","arguments":"RAW_COMMAND"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"exec-1","status":"completed","parsed_cmd":[
            {"type":"read","cmd":"RAW_COMMAND","path":"RAW_PATH"},
            {"type":"read","name":"RAW_FILE"},
            {"type":"search","query":"RAW_QUERY"},
            {"type":"list_files","path":"RAW_PATH"},
            {"type":"unknown","cmd":"RAW_COMMAND"},
            {"type":"future_category","cmd":"RAW_COMMAND"}
        ]}}}),
        json!({"type":"event_msg","payload":{"type":"exec_command_end","call_id":"exec-1","exit_code":0}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Done"}}),
    ]);
    assert_eq!(turns[0].activities.len(), 1);
    assert_eq!(
        turns[0].activities[0].actions,
        ["read_files", "search_files", "list_files", "run_commands"]
    );
    assert_eq!(
        turns[0].activities[0]
            .details
            .as_ref()
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .text,
        "RAW_COMMAND"
    );
    let serialized = serde_json::to_string(&turns).unwrap();
    assert!(!serialized.contains("RAW_PATH"));
    assert!(!serialized.contains("future_category"));
}

#[test]
fn codex_keeps_ordered_progress_and_tool_details_without_reasoning() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"fix it"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"commentary","content":[{"type":"Text","text":"Checking the implementation"}]}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"Checking the implementation"}]}}),
        json!({"type":"response_item","payload":{"type":"reasoning","content":[{"type":"text","text":"PRIVATE_REASONING"}]}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","raw_content":["PRIVATE_REASONING"]}}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"read-1","name":"Read","arguments":"RAW_ARGUMENTS"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"read-1","output":"RAW_OUTPUT"}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","phase":"commentary","message":"Applying the fix"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","call_id":"patch-1","name":"apply_patch","input":"RAW_PATCH"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"patch-1","output":{"isError":true,"content":"RAW_ERROR"}}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"read-2","name":"Read","arguments":"RAW_ARGUMENTS"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"read-2","output":"{\"exit_code\":0}"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Final conclusion"}}),
    ]);
    let turn = &turns[0];
    assert_eq!(
        turn.activities
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        [
            "Checking the implementation",
            "Read",
            "Applying the fix",
            "apply_patch",
            "Read"
        ]
    );
    assert_eq!(turn.activities[1].status, Some("completed"));
    assert_eq!(turn.activities[3].status, Some("failed"));
    let read = turn.activities[1].details.as_ref().unwrap();
    assert_eq!(read.input.as_ref().unwrap().text, "RAW_ARGUMENTS");
    assert_eq!(read.output.as_ref().unwrap().text, "RAW_OUTPUT");
    let patch = turn.activities[3].details.as_ref().unwrap();
    assert_eq!(patch.input.as_ref().unwrap().text, "RAW_PATCH");
    assert_eq!(patch.error.as_ref().unwrap().text, "RAW_ERROR");
    assert_eq!(
        turn.activities[4].details.as_ref().unwrap().exit_code,
        Some(0)
    );
    assert_eq!(
        turn.final_message.as_ref().unwrap().text,
        "Final conclusion"
    );
    let serialized = serde_json::to_string(turn).unwrap();
    assert!(!serialized.contains("PRIVATE_REASONING"));
}

#[test]
fn traecli_item_projections_merge_tool_start_and_end_by_call_id() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"traecli-turn"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":"run checks"}]}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"commentary","content":[{"type":"Text","text":"Running checks"}]}}}),
        json!({"type":"event_msg","payload":{"type":"item_started","item":{"type":"CommandExecution","id":"exec-1","status":"in_progress"}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"exec-1","status":"completed","exit_code":1,"command":"RAW_COMMAND","stdout":"RAW_STDOUT"}}}),
        json!({"type":"event_msg","payload":{"type":"exec_command_end","call_id":"exec-1","status":"completed","exit_code":1}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"FileChange","id":"patch-1","status":"completed","changes":{"RAW_PATH":"RAW_DIFF"}}}}),
        json!({"type":"event_msg","payload":{"type":"patch_apply_end","call_id":"patch-1","success":true}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Check failed; fixed the file."}}),
    ]);
    assert_eq!(turns[0].id, "traecli-turn");
    assert_eq!(turns[0].activities.len(), 3);
    assert_eq!(turns[0].activities[1].status, Some("failed"));
    assert_eq!(turns[0].activities[2].status, Some("completed"));
    let details = turns[0].activities[1].details.as_ref().unwrap();
    assert_eq!(details.command.as_ref().unwrap().text, "RAW_COMMAND");
    assert_eq!(details.output.as_ref().unwrap().text, "RAW_STDOUT");
    assert_eq!(details.exit_code, Some(1));
}

#[test]
fn claude_promotes_text_before_a_tool_to_progress_and_keeps_only_the_conclusion() {
    use serde_json::json;
    let turns = claude_turns(vec![
            json!({"type":"user","uuid":"u1","message":{"content":"fix tests"}}),
            json!({"type":"assistant","message":{"stop_reason":null,"content":[{"type":"text","text":"Inspecting tests"}]}}),
            json!({"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"thinking","thinking":"PRIVATE_REASONING"},{"type":"tool_use","id":"tool-1","name":"Bash","input":{"command":"RAW_COMMAND"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","is_error":true,"content":"RAW_ERROR"}]}}),
            json!({"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"text","text":"Fixing the failure"},{"type":"tool_use","id":"tool-2","name":"Edit","input":"RAW_PATCH"}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-2","content":"RAW_OUTPUT"}]}}),
            json!({"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Tests fixed"}]}}),
            json!({"type":"user","uuid":"u2","message":{"content":"next request"}}),
        ].into_iter());
    assert_eq!(turns.len(), 2);
    assert_eq!(
        turns[0]
            .activities
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["Inspecting tests", "Bash", "Fixing the failure", "Edit"]
    );
    assert_eq!(turns[0].activities[1].status, Some("failed"));
    assert_eq!(turns[0].activities[3].status, Some("completed"));
    let details = turns[0].activities[1].details.as_ref().unwrap();
    assert_eq!(details.command.as_ref().unwrap().text, "RAW_COMMAND");
    assert_eq!(details.error.as_ref().unwrap().text, "RAW_ERROR");
    assert_eq!(
        turns[0].activities[3]
            .details
            .as_ref()
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .text,
        "RAW_OUTPUT"
    );
    assert_eq!(turns[0].final_message.as_ref().unwrap().text, "Tests fixed");
    assert_eq!(turns[1].status, "in_progress");
    assert!(turns[1].activities.is_empty());
    let serialized = serde_json::to_string(&turns).unwrap();
    assert!(!serialized.contains("PRIVATE_REASONING"));
}

#[test]
fn unfinished_and_interrupted_tools_do_not_claim_success() {
    use serde_json::json;
    let records = vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"run"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"exec_command"}}),
    ];
    let turns = parse_records(records.clone());
    assert_eq!(turns[0].activities[0].status, Some("in_progress"));
    assert!(turns[0].final_message.is_none());
    for (event, expected) in [
        ("turn_aborted", "interrupted"),
        ("task_complete", "unknown"),
        ("error", "unknown"),
    ] {
        let mut records = records.clone();
        records.push(json!({"type":"event_msg","payload":{"type":event}}));
        assert_eq!(
            parse_records(records)[0].activities[0].status,
            Some(expected)
        );
    }
}

#[test]
fn command_details_merge_arguments_and_native_execution_events() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"check files"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"cmd","name":"exec_command","arguments":"{\"cmd\":\"rg needle missing\",\"workdir\":\"/workspace\"}"}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{
            "type":"CommandExecution","id":"cmd","status":"completed",
            "command":["/bin/bash","-lc","rg needle missing"],"cwd":"/workspace",
            "stdout":"checking files\n","stderr":"rg: missing: No such file or directory\n",
            "aggregated_output":"checking files\nrg: missing: No such file or directory\n",
            "exit_code":2,"duration":{"secs":1,"nanos":250000000}
        }}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"cmd","output":""}}),
    ]);
    assert_eq!(turns[0].activities.len(), 1);
    let tool = &turns[0].activities[0];
    assert_eq!(tool.status, Some("failed"));
    let details = tool.details.as_ref().unwrap();
    assert_eq!(details.command.as_ref().unwrap().text, "rg needle missing");
    assert_eq!(details.cwd.as_ref().unwrap().text, "/workspace");
    assert!(details.input.as_ref().unwrap().text.contains("workdir"));
    assert_eq!(details.output.as_ref().unwrap().text, "checking files\n");
    assert_eq!(
        details.error.as_ref().unwrap().text,
        "rg: missing: No such file or directory\n"
    );
    assert_eq!(details.exit_code, Some(2));
    assert_eq!(details.duration_ms, Some(1250));
}

#[test]
fn tool_outputs_support_json_text_envelopes_and_missing_details() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"run"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"json","name":"exec_command","arguments":{"cmd":"npm test"}}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"json","output":"{\"exit_code\":1,\"output\":\"test failed\",\"wall_time_seconds\":0.25}"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"text","name":"exec_command"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"text","output":"Wall time: 1 second\nProcess exited with code 127\nFinal output:\ncommand not found"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"legacy","name":"exec_command"}}),
    ]);
    let activities = &turns[0].activities;
    assert_eq!(activities[0].status, Some("failed"));
    let details = activities[0].details.as_ref().unwrap();
    assert_eq!(details.output.as_ref().unwrap().text, "test failed");
    assert_eq!(details.duration_ms, Some(250));
    assert_eq!(details.exit_code, Some(1));
    assert_eq!(activities[1].status, Some("failed"));
    let details = activities[1].details.as_ref().unwrap();
    assert_eq!(details.exit_code, Some(127));
    assert!(
        details
            .output
            .as_ref()
            .unwrap()
            .text
            .ends_with("command not found")
    );
    assert!(
        !serde_json::to_value(&activities[2])
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("details")
    );
}

#[test]
fn long_tool_payloads_keep_unicode_boundaries_and_failure_tail() {
    use serde_json::json;
    let command = format!("start {} end", "命令".repeat(20_000));
    let output = format!(
        "first line\n{}\nerror: final failure",
        "输出".repeat(20_000)
    );
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"run"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"long","name":"exec_command","arguments":{"cmd":command}}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"long","output":{"exit_code":1,"output":output}}}),
    ]);
    let details = turns[0].activities[0].details.as_ref().unwrap();
    let command = details.command.as_ref().unwrap();
    assert!(command.truncated);
    assert!(command.text.starts_with("start "));
    assert!(command.text.ends_with(" end"));
    let output = details.output.as_ref().unwrap();
    assert!(output.truncated);
    assert!(output.text.starts_with("first line\n"));
    assert!(output.text.ends_with("error: final failure"));
    assert!(output.text.chars().count() < 16_100);
}

#[test]
fn generic_tool_inputs_and_text_results_exclude_binary_content() {
    use serde_json::json;
    let turns = parse_records(vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"search"}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"search","name":"search_files","arguments":{"query":"needle","path":"/workspace/src"}}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"search","output":{"content":[{"type":"text","text":"found src/main.rs"},{"type":"image","data":"BINARY_IMAGE_DATA"}]}}}),
        json!({"type":"response_item","payload":{"type":"function_call","call_id":"list","name":"list_files","arguments":{"path":"src"}}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"list","output":[{"path":"src/main.rs"},{"path":"src/lib.rs"}]}}),
    ]);
    let details = turns[0].activities[0].details.as_ref().unwrap();
    assert!(details.input.as_ref().unwrap().text.contains("needle"));
    assert!(
        details
            .input
            .as_ref()
            .unwrap()
            .text
            .contains("/workspace/src")
    );
    assert_eq!(details.output.as_ref().unwrap().text, "found src/main.rs");
    let output = &turns[0].activities[1]
        .details
        .as_ref()
        .unwrap()
        .output
        .as_ref()
        .unwrap()
        .text;
    assert!(output.contains("src/main.rs"));
    assert!(output.contains("src/lib.rs"));
    assert!(
        !serde_json::to_string(&turns)
            .unwrap()
            .contains("BINARY_IMAGE_DATA")
    );
}

#[test]
fn activity_limits_preserve_unicode_prompts_and_conclusions() {
    let mut draft = TurnDraft::new("long".to_owned());
    draft.set_user(
        SnapshotMessage {
            text: "full prompt".to_owned(),
            timestamp: None,
        },
        1,
    );
    for index in 0..MAX_TURN_ACTIVITIES + 3 {
        draft.add_commentary(SnapshotMessage {
            text: format!("{index}{}", "描述".repeat(MAX_ACTIVITY_TEXT)),
            timestamp: None,
        });
    }
    draft.set_final(
        SnapshotMessage {
            text: "full conclusion".to_owned(),
            timestamp: None,
        },
        4,
    );
    let turn = draft.finish().unwrap();
    assert!(turn.activities_truncated);
    assert_eq!(turn.activities.len(), MAX_TURN_ACTIVITIES);
    assert!(turn.activities[0].text.starts_with('3'));
    assert_eq!(
        turn.activities[0].text.chars().count(),
        MAX_ACTIVITY_TEXT + 1
    );
    assert!(turn.activities[0].text.ends_with('…'));
    assert_eq!(turn.user.text, "full prompt");
    assert_eq!(turn.final_message.unwrap().text, "full conclusion");
}

#[test]
fn legacy_transcripts_allow_whitespace_and_intermediate_messages() {
    let (_directory, path) = write(
        r#"{"type": "message", "role": "user", "content": "hello"}
{"type": "message", "role": "assistant", "content": "Let me check"}
{"type": "response_item", "payload": {"type": "function_call", "call_id": "read-1", "name": "Read"}}
{"type": "response_item", "payload": {"type": "function_call_output", "call_id": "read-1", "output": "ok"}}
{"type": "message", "role": "assistant", "content": "Done"}
{"type": "event_msg", "payload": {"type": "task_complete"}}
{"type":"response_item","payload":
"#,
    );
    let turns = parse_codex_like(&path).unwrap();
    assert_eq!(turns[0].activities.len(), 2);
    assert_eq!(turns[0].activities[0].text, "Let me check");
    assert_eq!(turns[0].final_message.as_ref().unwrap().text, "Done");
}

#[test]
fn codex_uses_turn_complete_final_and_removes_projections() {
    let (_directory, path) = write(
        r#"{"timestamp":"2026-09-01T00:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-09-01T00:00:01Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":"hello"}]}}}
{"timestamp":"2026-09-01T00:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"hello"}}
{"timestamp":"2026-09-01T00:00:03Z","type":"event_msg","payload":{"type":"agent_message","message":"working","phase":"commentary"}}
{"timestamp":"2026-09-01T00:00:04Z","type":"event_msg","payload":{"type":"agent_message","message":"draft final","phase":"final_answer"}}
{"timestamp":"2026-09-01T00:00:05Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":"authoritative final"}}
"#,
    );
    let turns = parse_codex_like(&path).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.text, "hello");
    assert_eq!(
        turns[0].final_message.as_ref().unwrap().text,
        "authoritative final"
    );
    assert_eq!(turns[0].status, "completed");
}

#[test]
fn codex_task_completion_settles_all_inputs_received_during_the_task() {
    use serde_json::json;
    for (event, expected, tool_status) in [
        ("task_complete", "completed", "unknown"),
        ("turn_complete", "completed", "unknown"),
        ("turn_aborted", "interrupted", "interrupted"),
        ("task_aborted", "interrupted", "interrupted"),
        ("error", "failed", "unknown"),
    ] {
        let records = vec![
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"task-1"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":"original request"}}),
            json!({"type":"response_item","payload":{"type":"function_call","call_id":"pending","name":"exec_command"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":"additional instruction"}}),
        ];
        let active = parse_records(records.clone());
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|turn| turn.status == "in_progress"));

        let mut records = records;
        records.push(json!({"type":"event_msg","payload":{"type":event,"turn_id":"task-1","last_agent_message":"Combined answer"}}));
        let turns = parse_records(records.clone());
        assert_eq!(turns.len(), 2);
        assert!(turns.iter().all(|turn| turn.status == expected), "{event}");
        assert_eq!(turns[0].user.text, "original request");
        assert_eq!(turns[1].user.text, "additional instruction");
        assert!(turns[0].final_message.is_none());
        assert_eq!(turns[0].activities[0].status, Some(tool_status));
        if expected == "completed" {
            assert_eq!(turns[1].final_message.as_ref().unwrap().text, "Combined answer");
        }

        records.extend([
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"task-2"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":"next request"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"task-2","last_agent_message":"Next answer"}}),
        ]);
        let turns = parse_records(records);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].status, expected);
        assert_eq!(turns[1].status, expected);
        assert_eq!(turns[2].status, "completed");
    }
}

#[test]
fn codex_closes_previous_turn_when_next_turn_starts_without_completion() {
    use serde_json::json;
    for start_event in ["task_started", "turn_started"] {
        let turns = parse_records(vec![
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"first"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"first request"}}),
            json!({"type":"response_item","payload":{"type":"function_call","call_id":"pending","name":"exec_command"}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"final_answer","content":[{"type":"Text","text":"First answer"}]}}}),
            json!({"type":"event_msg","payload":{"type":start_event,"turn_id":"second"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"second request"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Second answer"}}),
            json!({"type":"event_msg","payload":{"type":start_event,"turn_id":"third"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"third request"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Third answer"}}),
        ]);
        assert_eq!(turns.len(), 3);
        assert!(turns.iter().all(|turn| turn.status == "completed"));
        assert_eq!(turns[0].final_message.as_ref().unwrap().text, "First answer");
        assert_eq!(turns[0].activities[0].status, Some("unknown"));
    }
}

#[test]
fn codex_next_turn_interrupts_unfinished_work_and_preserves_failures() {
    use serde_json::json;
    for with_start_event in [false, true] {
        for failed in [false, true] {
            let mut records = vec![
                json!({"type":"event_msg","payload":{"type":"user_message","message":"first request"}}),
                json!({"type":"response_item","payload":{"type":"function_call","call_id":"pending","name":"exec_command"}}),
            ];
            if failed {
                records.push(json!({"type":"event_msg","payload":{"type":"agent_message","phase":"final_answer","message":"Failed to finish"}}));
                records.push(json!({"type":"event_msg","payload":{"type":"error"}}));
            }
            if with_start_event {
                records.push(json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"second"}}));
            }
            records.push(json!({"type":"event_msg","payload":{"type":"user_message","message":"second request"}}));
            let turns = parse_records(records);
            assert_eq!(turns.len(), 2);
            assert_eq!(turns[0].status, if failed { "failed" } else { "interrupted" });
            assert_eq!(turns[0].activities[0].status, Some(if failed { "unknown" } else { "interrupted" }));
            assert_eq!(turns[1].status, "in_progress");
        }
    }
}

#[test]
fn codex_keeps_unfinished_user_turn_without_injected_rows() {
    let (_directory, path) = write(
        r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"type":"event_msg","payload":{"type":"user_message","message":"<system-reminder>noise</system-reminder>"}}
{"type":"event_msg","payload":{"type":"user_message","message":"real prompt"}}
"#,
    );
    let turns = parse_codex_like(&path).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.text, "real prompt");
    assert_eq!(turns[0].status, "in_progress");
    assert!(turns[0].final_message.is_none());
}

#[test]
fn codex_supports_unwrapped_legacy_messages() {
    let (_directory, path) = write(
        r#"{"timestamp":"2026-09-01T00:00:00Z","type":"message","role":"user","content":[{"type":"input_text","text":"legacy prompt"}]}
{"timestamp":"2026-09-01T00:00:01Z","type":"message","role":"assistant","content":[{"type":"output_text","text":"legacy final"}]}
"#,
    );
    let turns = parse_codex_like(&path).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.text, "legacy prompt");
    assert_eq!(
        turns[0].final_message.as_ref().unwrap().text,
        "legacy final"
    );
}

#[test]
fn claude_follows_last_prompt_branch_and_ignores_tool_rows() {
    let (_directory, path) = write(
        r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"content":"first"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"first final"}]}}
{"type":"user","uuid":"old-u","parentUuid":"a1","message":{"content":"abandoned"}}
{"type":"assistant","uuid":"old-a","parentUuid":"old-u","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"old final"}]}}
{"type":"user","uuid":"u2","parentUuid":"a1","message":{"content":"current"}}
{"type":"assistant","uuid":"tool","parentUuid":"u2","message":{"stop_reason":"tool_use","content":[{"type":"text","text":"not final"},{"type":"tool_use"}]}}
{"type":"user","uuid":"result","parentUuid":"tool","message":{"content":[{"type":"tool_result","content":"ok"}]}}
{"type":"assistant","uuid":"a2","parentUuid":"result","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"current final"}]}}
{"type":"last-prompt","sessionId":"claude-1","leafUuid":"a2"}
"#,
    );
    let turns = parse_claude(&path, "claude-1").unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].user.text, "first");
    assert_eq!(turns[1].user.text, "current");
    assert_eq!(
        turns[1].final_message.as_ref().unwrap().text,
        "current final"
    );
}

#[test]
fn enum_snapshot_dispatch_preserves_trusted_paths_and_agent_identity() {
    for agent in [Agent::Codex, Agent::TraeCli, Agent::Claude] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("history");
        fs::create_dir(&root).unwrap();
        let path = root.join("session.jsonl");
        let contents = if agent == Agent::Claude {
            r#"{"type":"user","uuid":"user-1","sessionId":"session","message":{"content":"hello"}}"#
        } else {
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"}}"#
        };
        fs::write(&path, contents).unwrap();
        let locator = AgentSessionLocator {
            agent: agent.id(),
            session_id: "session".into(),
            title: "Title".into(),
            cwd: directory.path().into(),
            transcript_path: path,
            trusted_root: root.clone(),
        };
        let snapshot = read(locator.clone()).unwrap();
        assert_eq!(snapshot.agent, agent.id());
        assert_eq!(snapshot.turns[0].user.text, "hello");
        let mut wrong_agent = locator.clone();
        wrong_agent.agent = if agent == Agent::Codex {
            "traecli"
        } else {
            "codex"
        };
        assert!(matches!(
            agent.sessions().unwrap().read_snapshot(wrong_agent),
            Err(SnapshotError::Invalid(_))
        ));
        let outside = directory.path().join("outside.jsonl");
        fs::write(&outside, contents).unwrap();
        let mut escaped = locator.clone();
        escaped.transcript_path = outside.clone();
        assert!(matches!(
            read(escaped.clone()),
            Err(SnapshotError::Invalid(_))
        ));
        #[cfg(unix)]
        {
            let link = root.join("linked.jsonl");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            escaped.transcript_path = link;
            assert!(matches!(read(escaped), Err(SnapshotError::Invalid(_))));
        }
        fs::remove_file(&locator.transcript_path).unwrap();
        assert!(matches!(read(locator), Err(SnapshotError::NotFound)));
    }
}
