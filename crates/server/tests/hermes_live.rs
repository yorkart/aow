//! Read-only verification against an existing Hermes process and session.
//! Run explicitly with AOW_HERMES_TEST_PID, AOW_HERMES_TEST_SESSION_ID and
//! AOW_HERMES_TEST_RUNTIME_ID. Optional AOW_HERMES_TEST_EXPORT exports the public
//! snapshot to a private temporary directory for frontend verification.
use std::{collections::BTreeMap, path::PathBuf};

use aow_agents::{
    Agent,
    interactive::AgentInteractive,
    process::{ProcessInfo, recognize_process},
    sessions::{
        AgentSessionProvider, SessionEnvironment, SessionRoots,
        tail::SessionTail,
        tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
    },
};
use aow_terminald_client::TerminaldClient;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires an idle Hermes process/session/runtime with /status visible; performs no terminal input"]
async fn existing_hermes_process_session_and_screen_agree() -> anyhow::Result<()> {
    let pid: i32 = std::env::var("AOW_HERMES_TEST_PID")?.parse()?;
    let session_id = std::env::var("AOW_HERMES_TEST_SESSION_ID")?;
    let runtime_id = std::env::var("AOW_HERMES_TEST_RUNTIME_ID")?;
    let process = aow_process::info(pid)?;
    let cwd = aow_process::cwd(pid)?;
    let command = aow_process::command(pid);
    let arguments: Vec<_> = command
        .arguments
        .split(|b| *b == 0)
        .map(|arg| std::str::from_utf8(arg).unwrap_or(""))
        .collect();
    assert_eq!(
        recognize_process(&ProcessInfo::new(
            command.executable.as_deref().and_then(|path| path.to_str()),
            &arguments,
        )),
        Some(Agent::Hermes)
    );
    let bytes = aow_process::environment(pid, &process.start_time)?;
    let environment: SessionEnvironment = bytes
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let (key, value) = std::str::from_utf8(entry).ok()?.split_once('=')?;
            (matches!(key, "HOME" | "HERMES_HOME") && !value.is_empty())
                .then(|| (key.to_owned(), PathBuf::from(value)))
        })
        .collect();
    let roots = SessionRoots::from_configuration(
        environment.get("HOME").expect("process HOME"),
        &environment,
    );
    let tracker = Agent::Hermes.session_tracking().unwrap();
    let target = SessionTarget::Id(session_id.clone());
    let provider = Agent::Hermes.sessions().unwrap();
    let root = provider.session_root(environment.get("HOME").unwrap(), &environment);
    let has_pid_lease = std::fs::read(root.join("runtime/active_sessions.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.get("entries").unwrap_or(&value).as_array().cloned())
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry["pid"].as_i64() == Some(i64::from(pid)))
        });
    assert_eq!(
        tracker
            .resolve_live_session(LiveSessionContext {
                pid: Some(pid),
                cwd: cwd.to_str().unwrap(),
                title: "",
                environment: &environment,
            })
            .await,
        if has_pid_lease {
            SessionResolution::Resolved(target.clone())
        } else {
            SessionResolution::NotFound
        }
    );
    // A manual selection still reads the exact supplied session when native
    // PID leases are disabled; never infer ownership from a unique cwd.
    let candidates = tracker.candidate_sessions(&target, &cwd, roots.clone());
    assert_eq!(candidates.len(), 1);
    let locator = &candidates[0];
    assert_eq!(locator.session_id, session_id);
    let session = provider
        .find_session(&roots, &session_id)
        .expect("native session");
    assert!(
        provider
            .list_sessions(&roots, &cwd)
            .iter()
            .any(|item| item.locator().session_id == session_id)
    );
    assert_eq!(
        provider.current_title(locator).as_deref(),
        Some(locator.title.as_str())
    );
    let snapshot = serde_json::to_value(provider.read_snapshot(locator.clone())?)?;

    // Compare the actual native rows with the adapter output. Do not log user
    // messages, tool payloads, command arguments or reasoning in test output.
    let native =
        Connection::open_with_flags(&locator.transcript_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = native.prepare("SELECT id, role, content, tool_call_id, tool_calls, timestamp, finish_reason, reasoning FROM messages WHERE session_id = ?1 AND active = 1 ORDER BY id")?;
    let records = statement
        .query_map([&session_id], |row| {
            Ok(NativeMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                tool_call_id: row.get(3)?,
                tool_calls: row.get(4)?,
                timestamp: row.get(5)?,
                finish_reason: row.get(6)?,
                reasoning: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let users: Vec<_> = records.iter().filter(|row| row.role == "user").collect();
    let turns = snapshot["turns"].as_array().expect("turns");
    assert_eq!(turns.len(), users.len());
    for (turn, user) in turns.iter().zip(&users) {
        assert_eq!(
            turn["user"]["text"].as_str(),
            user.content.as_deref().map(str::trim)
        );
    }
    let finals: Vec<_> = records
        .iter()
        .filter(|row| row.finish_reason.as_deref() == Some("stop") && row.tool_calls.is_none())
        .collect();
    let displayed_finals: Vec<_> = turns
        .iter()
        .filter_map(|turn| turn["final"]["text"].as_str())
        .collect();
    assert_eq!(
        displayed_finals,
        finals
            .iter()
            .map(|row| row.content.as_deref().unwrap().trim())
            .collect::<Vec<_>>()
    );
    assert_eq!(snapshot["status"], "completed");
    let tools: BTreeMap<_, _> = turns
        .iter()
        .flat_map(|turn| turn["activities"].as_array().unwrap())
        .filter(|activity| activity["kind"] == "tool")
        .map(|activity| (activity["id"].as_str().unwrap(), activity))
        .collect();
    let calls: Vec<Value> = records
        .iter()
        .filter_map(|row| row.tool_calls.as_deref())
        .map(serde_json::from_str::<Vec<Value>>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(tools.len(), calls.len());
    for call in &calls {
        let activity = tools[format!("tool-{}", call["id"].as_str().unwrap()).as_str()];
        assert_eq!(activity["text"], call["function"]["name"]);
        assert_ne!(activity["status"], "in_progress");
        assert!(activity.get("details").is_some());
        let arguments: Value =
            serde_json::from_str(call["function"]["arguments"].as_str().unwrap())?;
        if let Some(command) = arguments.get("command") {
            assert_eq!(&activity["details"]["command"]["text"], command);
        }
        let result = records
            .iter()
            .find(|row| row.role == "tool" && row.tool_call_id.as_deref() == call["id"].as_str())
            .expect("native tool result");
        if let Some(exit_code) = result
            .content
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|output| output["exit_code"].as_i64())
        {
            assert_eq!(activity["details"]["exit_code"], exit_code);
            assert_eq!(
                activity["status"],
                if exit_code == 0 {
                    "completed"
                } else {
                    "failed"
                }
            );
        }
    }
    let serialized = snapshot.to_string();
    for reasoning in records
        .iter()
        .filter_map(|row| row.reasoning.as_deref())
        .filter(|text| text.len() >= 20)
    {
        assert!(
            !serialized.contains(reasoning),
            "internal reasoning appeared in the public snapshot"
        );
    }
    assert!(
        SessionTail::from_eof(locator.clone())?.poll()?.is_empty(),
        "attaching must not replay history"
    );

    // Replay only this session's original rows in a disposable WAL store, to
    // test completion notifications without modifying the user's live store.
    let replay_directory = tempfile::tempdir()?;
    let replay = Connection::open(replay_directory.path().join("state.db"))?;
    replay.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE sessions(id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT, started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT);
        CREATE TABLE messages(id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, content TEXT, tool_call_id TEXT, tool_calls TEXT, timestamp REAL, finish_reason TEXT, active INTEGER DEFAULT 1);")?;
    replay.execute(
        "INSERT INTO sessions(id, source, cwd, title, started_at) VALUES (?1, 'cli', ?2, ?3, 0)",
        params![session_id, locator.cwd.to_str(), locator.title],
    )?;
    let mut replay_locator = locator.clone();
    replay_locator.trusted_root = replay_directory.path().to_path_buf();
    replay_locator.transcript_path = replay_directory.path().join("state.db");
    let mut tail = SessionTail::from_eof(replay_locator)?;
    let mut events = Vec::new();
    for row in &records {
        replay.execute("INSERT INTO messages(id, session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![row.id, session_id, row.role, row.content, row.tool_call_id, row.tool_calls, row.timestamp, row.finish_reason])?;
        events.extend(tail.poll()?);
        assert!(
            tail.poll()?.is_empty(),
            "polling replayed an already delivered event"
        );
    }
    assert_eq!(events.len(), finals.len());
    for (event, final_row) in events.iter().zip(&finals) {
        assert_eq!(event.turn_id, Some(format!("hermes-{}", final_row.id)));
        assert_eq!(
            event.conclusion.as_deref(),
            final_row.content.as_deref().map(str::trim)
        );
    }

    let client = TerminaldClient::default_socket();
    let runtime = client.get(&runtime_id).await?.expect("existing runtime");
    assert_eq!(
        PathBuf::from(&runtime.cwd).canonicalize()?,
        cwd.canonicalize()?
    );
    let screen = client.screen(&runtime_id).await?.expect("current screen");
    assert!(
        screen.lines.iter().any(|line| line.contains(&session_id)),
        "show /status in the selected Hermes runtime to verify its native identity"
    );
    assert!(
        Agent::Hermes
            .interactive()
            .unwrap()
            .input_ready(&screen.lines),
        "the live Hermes CLI is not idle"
    );
    assert!(aow_process::same_process(pid, &process.start_time));
    let summary = json!({"session_id":session_id, "pid":pid, "cwd":cwd,
        "runtime_id":runtime_id, "input_ready":true, "turns":turns.len(),
        "completed_turns":turns.iter().filter(|turn| turn["status"] == "completed").count(),
        "interrupted_turns":turns.iter().filter(|turn| turn["status"] == "interrupted").count(),
        "tool_calls":tools.len(), "replayed_completion_events":events.len(),
        "failed_tools":tools.values().filter(|tool| tool["status"] == "failed").count(),
        "viewport_contains_session_id":true,
        "database":locator.transcript_path});
    if let Some(path) = std::env::var_os("AOW_HERMES_TEST_EXPORT") {
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::write(path.join("snapshot.json"), serde_json::to_vec(&snapshot)?)?;
        std::fs::write(path.join("session.json"), serde_json::to_vec(&session)?)?;
        std::fs::write(path.join("summary.json"), serde_json::to_vec(&summary)?)?;
    }
    eprintln!("{summary}");
    Ok(())
}

struct NativeMessage {
    id: i64,
    role: String,
    content: Option<String>,
    tool_call_id: Option<String>,
    tool_calls: Option<String>,
    timestamp: f64,
    finish_reason: Option<String>,
    reasoning: Option<String>,
}
