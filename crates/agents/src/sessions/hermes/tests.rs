use super::*;
use crate::sessions::{
    tail::SessionTail,
    tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
};
use serde_json::json;

struct Fixture {
    directory: tempfile::TempDir,
    connection: Connection,
    roots: SessionRoots,
}

#[test]
#[ignore = "requires AOW_HERMES_TEST_HOME and AOW_HERMES_TEST_CWD from an installed Hermes smoke run"]
fn reads_the_store_created_by_installed_hermes() {
    let root = PathBuf::from(std::env::var_os("AOW_HERMES_TEST_HOME").expect("test profile"));
    let cwd = PathBuf::from(std::env::var_os("AOW_HERMES_TEST_CWD").expect("test workspace"));
    let roots =
        SessionRoots::from_configuration(&root, &[("HERMES_HOME".into(), root.clone())].into());
    let sessions = Hermes.list_sessions(&roots, &cwd);
    assert!(!sessions.is_empty());
    for session in &sessions {
        let snapshot = serde_json::to_value(snapshot::read(session.locator()).unwrap()).unwrap();
        assert_eq!(snapshot["agent"], "hermes");
        assert_eq!(snapshot["status"], "completed");
        assert!(!snapshot["turns"].as_array().unwrap().is_empty());
        assert!(Hermes.find_session(&roots, &session.session_id).is_some());
        assert!(
            Agent::Hermes
                .sessions()
                .unwrap()
                .current_title(&session.locator())
                .is_some()
        );
        assert!(
            SessionTail::from_eof(session.locator())
                .unwrap()
                .poll()
                .unwrap()
                .is_empty()
        );
    }
    eprintln!(
        "Read {} installed Hermes session(s), including native transcript snapshots",
        sessions.len()
    );
}

#[test]
fn lists_symlinked_workspaces_and_excludes_delegated_children() {
    let fixture = Fixture::new();
    let cwd = fixture.directory.path().join("workspace");
    std::fs::create_dir(&cwd).unwrap();
    let alias = fixture.directory.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&cwd, &alias).unwrap();
    fixture.session("root", "cli", cwd.canonicalize().unwrap().to_str().unwrap());
    fixture.message("root", "user", "Question", None, None, None);
    fixture.session(
        "child",
        "cli",
        cwd.canonicalize().unwrap().to_str().unwrap(),
    );
    fixture.connection.execute("UPDATE sessions SET parent_session_id = 'root', model_config = '{\"_delegate_from\":\"root\"}' WHERE id = 'child'", []).unwrap();
    fixture.message("child", "user", "Delegated", None, None, None);
    #[cfg(unix)]
    assert_eq!(Hermes.list_sessions(&fixture.roots, &alias).len(), 1);
    assert!(Hermes.find_session(&fixture.roots, "child").is_none());
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let connection = Connection::open(directory.path().join("state.db")).unwrap();
        connection.execute_batch("
            PRAGMA journal_mode=WAL;
            CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT,
                started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT, model_config TEXT, archived INTEGER DEFAULT 0);
            CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT, role TEXT,
                content TEXT, tool_call_id TEXT, tool_calls TEXT, timestamp REAL, finish_reason TEXT,
                reasoning TEXT, active INTEGER DEFAULT 1, compacted INTEGER DEFAULT 0);
        ").unwrap();
        let environment = [("HERMES_HOME".into(), directory.path().to_path_buf())].into();
        let roots = SessionRoots::from_configuration(directory.path(), &environment);
        Self {
            directory,
            connection,
            roots,
        }
    }

    fn session(&self, id: &str, source: &str, cwd: &str) {
        self.connection.execute("INSERT INTO sessions (id, source, cwd, started_at) VALUES (?1, ?2, ?3, 1700000000.125)",
            params![id, source, cwd]).unwrap();
    }

    fn message(
        &self,
        id: &str,
        role: &str,
        text: &str,
        reason: Option<&str>,
        calls: Option<Value>,
        call_id: Option<&str>,
    ) {
        self.connection.execute("INSERT INTO messages (session_id, role, content, timestamp, finish_reason, tool_calls, tool_call_id, reasoning)
            VALUES (?1, ?2, ?3, COALESCE((SELECT MAX(timestamp) + 0.01 FROM messages), 1700000001.5), ?4, ?5, ?6, 'PRIVATE_REASONING')",
            params![id, role, text, reason, calls.map(|value| value.to_string()), call_id]).unwrap();
    }

    fn locator(&self, id: &str) -> AgentSessionLocator {
        Hermes.find_session(&self.roots, id).unwrap().locator()
    }

    fn snapshot(&self, id: &str) -> Value {
        serde_json::to_value(snapshot::read(self.locator(id)).unwrap()).unwrap()
    }

    // Same archive + reinsert contract as native archive_and_compact. Use new
    // timestamps too: a live CLI need not preserve the original row timestamp.
    fn compact(&self, id: &str, head: usize, tail: usize, merged: bool) {
        let ids: Vec<i64> = self
            .connection
            .prepare("SELECT id FROM messages WHERE session_id = ?1 AND active = 1 ORDER BY id")
            .unwrap()
            .query_map([id], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(head + tail <= ids.len());
        self.connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        self.connection.execute("UPDATE messages SET active = 0, compacted = 1 WHERE session_id = ?1 AND active = 1", [id]).unwrap();
        let clock = std::cell::Cell::new(
            self.connection
                .query_row("SELECT MAX(timestamp) + 100 FROM messages", [], |row| {
                    row.get::<_, f64>(0)
                })
                .unwrap(),
        );
        let copy = |row: i64, merged: bool| {
            self.connection.execute("INSERT INTO messages(session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason)
                SELECT session_id, role, CASE WHEN ?2 THEN
                    '[PRIOR CONTEXT — for reference only; not a new message]' || char(10) || content || char(10) ||
                    '[END OF PRIOR CONTEXT — COMPACTION SUMMARY BELOW]' || char(10) || '[CONTEXT SUMMARY]: older messages'
                    ELSE content END, tool_call_id, tool_calls, ?3, finish_reason
                FROM messages WHERE id = ?1", params![row, merged, clock.get()]).unwrap();
            clock.set(clock.get() + 1e-6);
        };
        for row in &ids[..head] {
            copy(*row, false);
        }
        if !merged {
            self.message(
                id,
                "user",
                "[CONTEXT SUMMARY]: older messages",
                None,
                None,
                None,
            );
            self.connection
                .execute(
                    "UPDATE messages SET timestamp = ?1 WHERE id = last_insert_rowid()",
                    [clock.get()],
                )
                .unwrap();
            clock.set(clock.get() + 1e-6);
        }
        for (index, row) in ids[ids.len() - tail..].iter().enumerate() {
            copy(*row, merged && index == 0);
        }
        self.connection.execute_batch("COMMIT").unwrap();
    }
}

#[test]
fn reads_native_wal_history_and_filters_workspace_sources_and_archives() {
    let fixture = Fixture::new();
    for (id, source, cwd) in [
        ("one", "cli", "/repo"),
        ("two", "tui", "/repo"),
        ("foreign", "cli", "/other"),
        ("gateway", "telegram", "/repo"),
        ("delegate", "tool", "/repo"),
        ("empty", "cli", "/repo"),
        ("archived", "cli", "/repo"),
    ] {
        fixture.session(id, source, cwd);
        if id != "empty" {
            fixture.message(id, "user", "First question", None, None, None);
        }
    }
    fixture
        .connection
        .execute("UPDATE sessions SET archived = 1 WHERE id = 'archived'", [])
        .unwrap();
    fixture
        .connection
        .execute(
            "UPDATE sessions SET title = ' Renamed  session ' WHERE id = 'one'",
            [],
        )
        .unwrap();
    let sessions = Hermes.list_sessions(&fixture.roots, Path::new("/repo"));
    assert_eq!(sessions.len(), 2);
    let session = Hermes.find_session(&fixture.roots, "one").unwrap();
    assert_eq!(session.title, "Renamed session");
    assert_eq!(session.created_at, "2023-11-14T22:13:20.125Z");
    assert_eq!(session.updated_at, "2023-11-14T22:13:21.500Z");
    fixture
        .connection
        .execute(
            "UPDATE sessions SET title = 'Latest title' WHERE id = 'one'",
            [],
        )
        .unwrap();
    assert_eq!(
        Agent::Hermes
            .sessions()
            .unwrap()
            .current_title(&session.locator())
            .as_deref(),
        Some("Latest title")
    );
    assert!(Hermes.find_session(&fixture.roots, "missing").is_none());
    assert!(Hermes.find_session(&fixture.roots, "delegate").is_none());
}

#[test]
fn respects_configured_home_and_sticky_profiles_without_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(".hermes");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("active_profile"), "work\n").unwrap();
    assert_eq!(
        home(directory.path(), &SessionEnvironment::new()),
        root.join("profiles/work")
    );
    let environment = [("HERMES_HOME".into(), directory.path().join("custom"))].into();
    assert_eq!(
        home(directory.path(), &environment),
        directory.path().join("custom")
    );
    std::fs::write(root.join("active_profile"), "../../escape").unwrap();
    assert_eq!(home(directory.path(), &SessionEnvironment::new()), root);
}

#[test]
fn configured_roots_follow_native_profiles_but_explicit_profiles_stay_pinned() {
    let directory = tempfile::tempdir().unwrap();
    for root in [
        directory.path().join(".hermes"),
        directory.path().join("custom"),
    ] {
        std::fs::create_dir_all(root.join("profiles/work")).unwrap();
        std::fs::write(root.join("active_profile"), "Work\n").unwrap();
        let environment = [("HERMES_HOME".into(), root.clone())].into();
        assert_eq!(
            home(directory.path(), &environment),
            root.join("profiles/work")
        );
        let environment = [("HERMES_HOME".into(), root.join("profiles/other"))].into();
        assert_eq!(
            home(directory.path(), &environment),
            root.join("profiles/other")
        );
        let environment = [("HERMES_HOME".into(), root.clone())].into();
        for invalid in ["../../escape", "_invalid", "", "default"] {
            std::fs::write(root.join("active_profile"), invalid).unwrap();
            assert_eq!(home(directory.path(), &environment), root);
        }
    }
}

#[test]
fn compaction_preserves_history_and_ids_without_replaying_stops() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    for n in 1..=4 {
        fixture.message("one", "user", &format!("Question {n}"), None, None, None);
        fixture.message(
            "one",
            "assistant",
            &format!("Answer {n}"),
            Some("stop"),
            None,
            None,
        );
    }
    let mut tail = SessionTail::from_eof(fixture.locator("one")).unwrap();
    let original = fixture.snapshot("one");
    fixture.compact("one", 2, 4, false);
    assert_eq!(fixture.snapshot("one")["turns"], original["turns"]);
    assert!(tail.poll().unwrap().is_empty());
    // A genuinely new identical turn must survive; content alone is not an ID.
    fixture.message("one", "user", "Question 4", None, None, None);
    fixture.message("one", "assistant", "Answer 4", Some("stop"), None, None);
    let before_second = fixture.snapshot("one");
    assert_eq!(before_second["turns"].as_array().unwrap().len(), 5);
    // The next compaction happens before the reader sees that new completion.
    fixture.compact("one", 2, 4, false);
    assert_eq!(fixture.snapshot("one")["turns"], before_second["turns"]);
    let events = tail.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].conclusion.as_deref(), Some("Answer 4"));
    assert!(tail.poll().unwrap().is_empty());
    assert!(
        SessionTail::from_eof(fixture.locator("one"))
            .unwrap()
            .poll()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn compaction_merged_into_a_tail_keeps_tools_and_honors_rewinds() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    fixture.message("one", "user", "Old question", None, None, None);
    fixture.message("one", "assistant", "Old answer", Some("stop"), None, None);
    fixture.message("one", "user", "Inspect", None, None, None);
    fixture.message(
        "one",
        "assistant",
        "Looking",
        Some("tool_calls"),
        Some(json!([
            {"id":"call", "function":{"name":"terminal", "arguments":"{\"command\":\"ls\"}"}}
        ])),
        None,
    );
    fixture.message(
        "one",
        "tool",
        "{\"output\":\"file\",\"exit_code\":0}",
        None,
        None,
        Some("call"),
    );
    fixture.message("one", "assistant", "Done", Some("stop"), None, None);
    let original = fixture.snapshot("one");
    let mut tail = SessionTail::from_eof(fixture.locator("one")).unwrap();
    fixture.compact("one", 0, 4, true);
    assert_eq!(fixture.snapshot("one")["turns"], original["turns"]);
    assert!(tail.poll().unwrap().is_empty());
    fixture.compact("one", 0, 4, true);
    assert_eq!(fixture.snapshot("one")["turns"], original["turns"]);
    assert!(tail.poll().unwrap().is_empty());
    // Native rewind marks only the new copies inactive, leaving the archived
    // source rows compacted=1. They must not resurrect the retracted turn.
    fixture
        .connection
        .execute(
            "UPDATE messages SET active = 0 WHERE session_id = 'one' AND compacted = 0",
            [],
        )
        .unwrap();
    let rewound = fixture.snapshot("one");
    assert_eq!(rewound["turns"].as_array().unwrap().len(), 1);
    assert_eq!(rewound["turns"][0], original["turns"][0]);
    fixture.message("one", "user", "Inspect", None, None, None);
    fixture.message("one", "assistant", "Done", Some("stop"), None, None);
    assert_eq!(
        fixture.snapshot("one")["turns"].as_array().unwrap().len(),
        2
    );
    assert_eq!(tail.poll().unwrap().len(), 1);
}

#[test]
fn compaction_keeps_snapshot_and_tail_budgets_after_removing_copies() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    let mut tail = SessionTail::from_eof(fixture.locator("one")).unwrap();
    for _ in 0..270 {
        fixture.message("one", "user", "Same question", None, None, None);
        fixture.message("one", "assistant", "Same answer", Some("stop"), None, None);
    }
    fixture.compact("one", 2, 532, false);
    let snapshot = fixture.snapshot("one");
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 200);
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(tail.poll().unwrap().len(), 256);
    assert_eq!(tail.poll().unwrap().len(), 14);
    assert!(tail.poll().unwrap().is_empty());
    let original_ids: Vec<_> = snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| turn["id"].clone())
        .collect();
    fixture.compact("one", 2, 528, false);
    let repeated = fixture.snapshot("one");
    assert_eq!(
        repeated["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| turn["id"].clone())
            .collect::<Vec<_>>(),
        original_ids
    );
    assert!(tail.poll().unwrap().is_empty());
}

#[test]
fn snapshots_map_tools_and_terminal_replies_and_hide_internal_reasoning() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    fixture.message("one", "system", "PRIVATE_SYSTEM", None, None, None);
    fixture.message("one", "user", "Inspect the repository", None, None, None);
    fixture.message("one", "assistant", "<think>PRIVATE_THOUGHT</think>Checking files", Some("tool_calls"),
        Some(json!([{"id":"call-1", "type":"function", "function":{"name":"terminal", "arguments":"{\"command\":\"ls\"}"}}])), None);
    fixture.message(
        "one",
        "tool",
        "{\"output\":\"a.rs\",\"exit_code\":0}",
        None,
        None,
        Some("call-1"),
    );
    fixture.message(
        "one",
        "assistant",
        "<REASONING_SCRATCHPAD>PRIVATE_PLAN</REASONING_SCRATCHPAD>Done",
        Some("stop"),
        None,
        None,
    );
    let snapshot = fixture.snapshot("one");
    assert_eq!(snapshot["status"], "completed");
    let turn = &snapshot["turns"][0];
    assert_eq!(turn["user"]["text"], "Inspect the repository");
    assert_eq!(turn["final"]["text"], "Done");
    assert_eq!(turn["activities"][0]["text"], "Checking files");
    assert_eq!(turn["activities"][1]["text"], "terminal");
    assert_eq!(turn["activities"][1]["status"], "completed");
    assert_eq!(turn["activities"][1]["details"]["command"]["text"], "ls");
    assert!(!snapshot.to_string().contains("PRIVATE_"));
    fixture.message("one", "user", "Next question", None, None, None);
    fixture.message("one", "assistant", "Still working", None, None, None);
    assert_eq!(fixture.snapshot("one")["status"], "in_progress");
}

#[test]
fn snapshots_follow_compression_but_exclude_rewound_rows_and_other_branches() {
    let fixture = Fixture::new();
    for id in ["parent", "child", "branch", "delegate", "stale"] {
        fixture.session(id, "cli", "/repo");
    }
    fixture.connection.execute("UPDATE sessions SET end_reason = 'compression', ended_at = 1700000002 WHERE id = 'parent'", []).unwrap();
    fixture
        .connection
        .execute(
            "UPDATE sessions SET parent_session_id = 'parent' WHERE id = 'child'",
            [],
        )
        .unwrap();
    fixture
        .connection
        .execute(
            "UPDATE sessions SET parent_session_id = 'parent', model_config = '{\"_branched_from\":\"parent\"}' WHERE id = 'branch'",
            [],
        )
        .unwrap();
    fixture.connection.execute("UPDATE sessions SET parent_session_id = 'parent', model_config = '{\"_delegate_from\":\"parent\"}' WHERE id = 'delegate'", []).unwrap();
    fixture.connection.execute("UPDATE sessions SET parent_session_id = 'parent', ended_at = 1700000004, end_reason = 'ws_orphan_reap' WHERE id = 'stale'", []).unwrap();
    fixture.message("parent", "user", "Old turn", None, None, None);
    fixture.message(
        "parent",
        "assistant",
        "Old answer",
        Some("stop"),
        None,
        None,
    );
    fixture.message("child", "user", "New turn", None, None, None);
    fixture.message(
        "child",
        "assistant",
        "Discarded answer",
        Some("stop"),
        None,
        None,
    );
    fixture
        .connection
        .execute(
            "UPDATE messages SET active = 0 WHERE content = 'Discarded answer'",
            [],
        )
        .unwrap();
    fixture.message("child", "assistant", "New answer", Some("stop"), None, None);
    fixture.message("branch", "user", "Other branch", None, None, None);
    fixture.message("delegate", "user", "Delegated task", None, None, None);
    fixture.message("stale", "user", "Stale sibling", None, None, None);
    fixture
        .connection
        .execute(
            "UPDATE messages SET timestamp = 1700000010 WHERE session_id = 'stale'",
            [],
        )
        .unwrap();
    let snapshot = fixture.snapshot("child");
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["turns"][1]["final"]["text"], "New answer");
    assert!(!snapshot.to_string().contains("Discarded"));
    assert!(!snapshot.to_string().contains("Other branch"));
    assert!(!snapshot.to_string().contains("Delegated task"));
    assert!(!snapshot.to_string().contains("Stale sibling"));
    let branch = fixture.snapshot("branch");
    assert_eq!(branch["turns"].as_array().unwrap().len(), 1);
    assert_eq!(branch["turns"][0]["user"]["text"], "Other branch");
}

#[test]
fn compressed_conversations_with_empty_roots_keep_the_latest_title_and_activity() {
    let fixture = Fixture::new();
    for id in ["root", "child", "unrelated"] {
        fixture.session(id, "cli", "/repo");
    }
    fixture.connection.execute("UPDATE sessions SET end_reason = 'compression', ended_at = 1700000002, title = 'Old title' WHERE id = 'root'", []).unwrap();
    fixture.connection.execute("UPDATE sessions SET parent_session_id = 'root', title = 'Current title' WHERE id = 'child'", []).unwrap();
    fixture.message("unrelated", "user", "Other task", None, None, None);
    fixture.message("child", "user", "Continuing the task", None, None, None);
    fixture.message("child", "assistant", "Finished", Some("stop"), None, None);
    fixture
        .connection
        .execute(
            "UPDATE messages SET timestamp = 1700000005 WHERE session_id = 'child'",
            [],
        )
        .unwrap();
    let sessions = Hermes.list_sessions(&fixture.roots, Path::new("/repo"));
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session_id, "root");
    assert_eq!(sessions[0].title, "Current title");
    assert_eq!(sessions[0].updated_at, "2023-11-14T22:13:25.000Z");
    assert_eq!(
        Hermes.current_title(&sessions[0].locator()).as_deref(),
        Some("Current title")
    );
    assert_eq!(
        fixture.snapshot("root")["turns"][0]["final"]["text"],
        "Finished"
    );
}

#[test]
fn structured_message_content_exposes_text_without_images_or_reasoning() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    let user = format!(
        "\0json:{}",
        json!([
            {"type":"text", "text":"Inspect this screenshot"},
            {"type":"image_url", "image_url":{"url":"data:image/png;base64,PRIVATE_IMAGE"}}
        ])
    );
    fixture.message("one", "user", &user, None, None, None);
    let reply = format!(
        "\0json:{}",
        json!([
            {"type":"thinking", "thinking":"PRIVATE_REASONING"},
            {"type":"text", "text":"<think>PRIVATE_THOUGHT</think>Visible reply"}
        ])
    );
    fixture.message("one", "assistant", &reply, Some("stop"), None, None);
    let sessions = Hermes.list_sessions(&fixture.roots, Path::new("/repo"));
    assert_eq!(sessions[0].title, "Inspect this screenshot");
    let snapshot = fixture.snapshot("one");
    assert_eq!(
        snapshot["turns"][0]["user"]["text"],
        "Inspect this screenshot"
    );
    assert_eq!(snapshot["turns"][0]["final"]["text"], "Visible reply");
    assert!(!snapshot.to_string().contains("PRIVATE_"));
}

#[test]
fn bounds_snapshots_to_recent_turns_and_validates_database_locators() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    for i in 0..205 {
        fixture.message("one", "user", &format!("Question {i}"), None, None, None);
    }
    let snapshot = fixture.snapshot("one");
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 200);
    assert_eq!(snapshot["truncated"], true);
    assert_eq!(snapshot["turns"][0]["user"]["text"], "Question 5");
    let mut locator = fixture.locator("one");
    let outside = tempfile::tempdir().unwrap();
    locator.trusted_root = outside.path().to_path_buf();
    assert!(snapshot::read(locator).is_err());
    let mut locator = fixture.locator("one");
    locator.session_id = "deleted".into();
    assert!(matches!(
        snapshot::read(locator),
        Err(snapshot::SnapshotError::NotFound)
    ));
    let mut locator = fixture.locator("one");
    locator.agent = "codex";
    assert!(snapshot::read(locator).is_err());
}

#[test]
fn sqlite_tail_skips_history_and_only_delivers_new_terminal_replies_once() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    fixture.session("other", "cli", "/other");
    fixture.message("one", "assistant", "Historical", Some("stop"), None, None);
    let mut tail = SessionTail::from_eof(fixture.locator("one")).unwrap();
    assert!(tail.poll().unwrap().is_empty());
    fixture.message("other", "assistant", "Foreign", Some("stop"), None, None);
    for reason in [
        None,
        Some("tool_calls"),
        Some("incomplete"),
        Some("length"),
        Some("verification_required"),
        Some("verify_hook_continue"),
    ] {
        fixture.message("one", "assistant", "Not final", reason, None, None);
    }
    fixture.message(
        "one",
        "assistant",
        "Tool request",
        Some("stop"),
        Some(json!([{"id":"call"}])),
        None,
    );
    fixture.message(
        "one",
        "assistant",
        "Malformed tool record",
        Some("stop"),
        Some(json!({"unexpected":"object"})),
        None,
    );
    assert!(tail.poll().unwrap().is_empty());
    fixture.message(
        "one",
        "assistant",
        "<think>SECRET</think>First conclusion",
        Some("stop"),
        None,
        None,
    );
    fixture.message(
        "one",
        "assistant",
        "Second conclusion",
        Some("end_turn"),
        None,
        None,
    );
    let events = tail.poll().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].conclusion.as_deref(), Some("First conclusion"));
    assert_eq!(events[1].conclusion.as_deref(), Some("Second conclusion"));
    assert!(tail.poll().unwrap().is_empty());
    let mut attached = SessionTail::from_eof(fixture.locator("one")).unwrap();
    assert!(attached.poll().unwrap().is_empty());
}

#[test]
fn sqlite_tail_recovers_after_database_replacement_without_replaying() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    fixture.message("one", "assistant", "Historical", Some("stop"), None, None);
    let mut tail = SessionTail::from_eof(fixture.locator("one")).unwrap();
    // VACUUM INTO makes a standalone, consistent database including WAL rows.
    let replacement = fixture.directory.path().join("replacement.db");
    fixture
        .connection
        .execute("VACUUM INTO ?1", [replacement.to_str().unwrap()])
        .unwrap();
    fixture
        .connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .unwrap();
    std::fs::rename(&replacement, fixture.directory.path().join("state.db")).unwrap();
    assert!(tail.poll().unwrap().is_empty());
    let next = Connection::open(fixture.directory.path().join("state.db")).unwrap();
    next.execute("INSERT INTO messages (session_id, role, content, timestamp, finish_reason) VALUES ('one','assistant','After replacement',1700000003,'stop')", []).unwrap();
    assert_eq!(
        tail.poll().unwrap()[0].conclusion.as_deref(),
        Some("After replacement")
    );
}

#[test]
fn sqlite_tail_follows_compression_without_replaying_history_or_other_branches() {
    let fixture = Fixture::new();
    fixture.session("root", "cli", "/repo");
    fixture.message("root", "user", "Start task", None, None, None);
    fixture.message("root", "assistant", "Historical", Some("stop"), None, None);
    let mut tail = SessionTail::from_eof(fixture.locator("root")).unwrap();
    for id in ["continuation", "branch"] {
        fixture.session(id, "cli", "/repo");
    }
    fixture.connection.execute("UPDATE sessions SET end_reason = 'compression', ended_at = 1700000002 WHERE id = 'root'", []).unwrap();
    fixture
        .connection
        .execute(
            "UPDATE sessions SET parent_session_id = 'root' WHERE id IN ('continuation', 'branch')",
            [],
        )
        .unwrap();
    fixture.connection.execute("UPDATE sessions SET model_config = '{\"_branched_from\":\"root\"}' WHERE id = 'branch'", []).unwrap();
    fixture.message(
        "continuation",
        "assistant",
        "After compression",
        Some("stop"),
        None,
        None,
    );
    fixture.message(
        "branch",
        "assistant",
        "Unrelated conclusion",
        Some("stop"),
        None,
        None,
    );
    let events = tail.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].conclusion.as_deref(), Some("After compression"));
    assert!(tail.poll().unwrap().is_empty());
    assert!(
        SessionTail::from_eof(fixture.locator("continuation"))
            .unwrap()
            .poll()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn live_association_requires_a_matching_pid_lease_even_with_one_open_session() {
    let fixture = Fixture::new();
    fixture.session("one", "cli", "/repo");
    fixture.message("one", "user", "Question", None, None, None);
    let environment = [
        ("HOME".into(), fixture.directory.path().to_path_buf()),
        ("HERMES_HOME".into(), fixture.directory.path().to_path_buf()),
    ]
    .into();
    let context = || LiveSessionContext {
        pid: Some(123),
        cwd: "/repo",
        title: "irrelevant",
        environment: &environment,
    };
    let tracker = Agent::Hermes.session_tracking().unwrap();
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::NotFound
    );
    fixture.session("two", "cli", "/repo");
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::NotFound
    );
    std::fs::create_dir(fixture.directory.path().join("runtime")).unwrap();
    std::fs::write(
        fixture
            .directory
            .path()
            .join("runtime/active_sessions.json"),
        json!({"entries":[{"pid":123,"surface":"cli","session_id":"one"}]}).to_string(),
    )
    .unwrap();
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::Resolved(SessionTarget::Id("one".into()))
    );
    assert!(
        tracker
            .candidate_sessions(
                &SessionTarget::Id("one".into()),
                Path::new("/other"),
                fixture.roots.clone()
            )
            .is_empty()
    );
    fixture
        .connection
        .execute(
            "UPDATE sessions SET ended_at = 1700000003 WHERE id = 'one'",
            [],
        )
        .unwrap();
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::NotFound
    );
}
