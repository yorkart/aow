use super::claude::*;
use super::codex_like::*;
use super::*;
use rusqlite::{Connection, params};
use std::fs;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn roots(directory: &tempfile::TempDir) -> SessionRoots {
    SessionRoots {
        claude: directory.path().join("claude"),
        codex: directory.path().join("codex"),
        traecli: directory.path().join("traecli"),
        hermes: directory.path().join("hermes"),
    }
}

fn create_state_db(home: &Path) -> Connection {
    fs::create_dir_all(home).unwrap();
    let connection = Connection::open(home.join(STATE_DB_FILENAME)).unwrap();
    connection
        .execute_batch(
            "
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    rollout_path TEXT NOT NULL,
    cwd TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    archived INTEGER NOT NULL,
    first_user_message TEXT NOT NULL,
    thread_source TEXT,
    source TEXT,
    agent_nickname TEXT,
    agent_role TEXT
);
",
        )
        .unwrap();
    connection
}

#[allow(clippy::too_many_arguments)]
fn insert_thread(
    connection: &Connection,
    id: &str,
    rollout_path: &Path,
    cwd: &Path,
    title: &str,
    updated_at_ms: i64,
    archived: bool,
    first_user_message: &str,
    thread_source: Option<&str>,
    source: Option<&str>,
    agent_nickname: Option<&str>,
    agent_role: Option<&str>,
) {
    connection
        .execute(
            "
INSERT INTO threads (
    id, rollout_path, cwd, title, created_at, updated_at, updated_at_ms,
    archived, first_user_message, thread_source, source, agent_nickname, agent_role
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
",
            params![
                id,
                rollout_path.to_string_lossy(),
                cwd.to_string_lossy(),
                title,
                1_725_100_000_i64,
                updated_at_ms / 1_000,
                updated_at_ms,
                i64::from(archived),
                first_user_message,
                thread_source,
                source,
                agent_nickname,
                agent_role,
            ],
        )
        .unwrap();
}

#[test]
fn sqlite_provider_lists_only_top_level_sessions_in_the_requested_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("repo");
    let other_worktree = directory.path().join("other-repo");
    let roots = roots(&directory);
    let rollout = roots.codex.join("sessions/2026/09/01/visible.jsonl");
    let connection = create_state_db(&roots.codex);
    insert_thread(
        &connection,
        "visible",
        &rollout,
        &worktree,
        "Visible title",
        1_725_100_100_000,
        false,
        "prompt",
        Some("user"),
        None,
        None,
        None,
    );
    insert_thread(
        &connection,
        "other-workspace",
        &rollout,
        &other_worktree,
        "Other title",
        1_725_100_200_000,
        false,
        "prompt",
        Some("user"),
        None,
        None,
        None,
    );
    insert_thread(
        &connection,
        "archived",
        &rollout,
        &worktree,
        "Archived",
        1_725_100_300_000,
        true,
        "prompt",
        Some("user"),
        None,
        None,
        None,
    );
    insert_thread(
        &connection,
        "empty-prompt",
        &rollout,
        &worktree,
        "Empty",
        1_725_100_400_000,
        false,
        "",
        Some("user"),
        None,
        None,
        None,
    );
    insert_thread(
        &connection,
        "thread-subagent",
        &rollout,
        &worktree,
        "Worker",
        1_725_100_500_000,
        false,
        "prompt",
        Some("subagent"),
        None,
        None,
        None,
    );
    insert_thread(
        &connection,
        "source-subagent",
        &rollout,
        &worktree,
        "Legacy worker",
        1_725_100_600_000,
        false,
        "prompt",
        None,
        Some(r#"{"subagent":{}}"#),
        None,
        None,
    );
    insert_thread(
        &connection,
        "nicknamed-subagent",
        &rollout,
        &worktree,
        "Named worker",
        1_725_100_700_000,
        false,
        "prompt",
        None,
        Some("cli"),
        Some("worker"),
        None,
    );
    insert_thread(
        &connection,
        "role-subagent",
        &rollout,
        &worktree,
        "Role worker",
        1_725_100_800_000,
        false,
        "prompt",
        None,
        Some("cli"),
        None,
        Some("explorer"),
    );

    let sessions = list_sessions(&worktree, Some("codex"), roots);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].agent, "codex");
    assert_eq!(sessions[0].session_id, "visible");
    assert_eq!(sessions[0].title, "Visible title");
    assert_eq!(sessions[0].cwd, worktree.to_string_lossy());
    assert_eq!(sessions[0].transcript_path, rollout);
}

#[test]
fn sqlite_provider_finds_session_by_id_without_a_workspace_constraint() {
    let directory = tempfile::tempdir().unwrap();
    let other_worktree = directory.path().join("other-repo");
    let roots = roots(&directory);
    let rollout = roots.codex.join("sessions/2026/09/01/automation.jsonl");
    let connection = create_state_db(&roots.codex);
    insert_thread(
        &connection,
        "automation-session",
        &rollout,
        &other_worktree,
        "Automation title",
        1_725_100_100_000,
        false,
        "prompt",
        Some("user"),
        None,
        None,
        None,
    );

    let session = find_session("codex", "automation-session", roots.clone()).unwrap();
    assert_eq!(session.cwd, other_worktree.to_string_lossy());
    assert_eq!(session.transcript_path, rollout);
    assert_eq!(session.trusted_root, roots.codex.join("sessions"));
    assert!(find_session("codex", "missing", roots.clone()).is_none());
    assert!(find_session("traecli", "automation-session", roots).is_none());
}

#[test]
fn codex_and_trae_use_native_names_across_schema_versions() {
    for agent in [Agent::Codex, Agent::TraeCli] {
        let directory = tempfile::tempdir().unwrap();
        let roots = roots(&directory);
        let home = if agent == Agent::Codex {
            &roots.codex
        } else {
            &roots.traecli
        };
        let cwd = directory.path().join("repo");
        let connection = create_state_db(home);
        insert_thread(
            &connection,
            "named",
            &home.join("sessions/named.jsonl"),
            &cwd,
            "first prompt",
            1_725_100_000_000,
            false,
            "first prompt",
            Some("cli"),
            None,
            None,
            None,
        );
        write(
            &home.join("session_index.jsonl"),
            "{\"id\":\"named\",\"thread_name\":\"old name\"}\n{\"id\":\"named\",\"thread_name\":\"renamed session\"}\n{partial",
        );
        assert_eq!(
            list_sessions(&cwd, Some(agent.id()), roots.clone())[0].title,
            "renamed session"
        );
        assert_eq!(
            find_session(agent.id(), "named", roots.clone())
                .unwrap()
                .title,
            "renamed session"
        );
        connection
            .execute_batch(
                "ALTER TABLE threads ADD COLUMN name TEXT; UPDATE threads SET name = 'native name'",
            )
            .unwrap();
        assert_eq!(
            list_sessions(&cwd, Some(agent.id()), roots.clone())[0].title,
            "native name"
        );
        assert_eq!(
            find_session(agent.id(), "named", roots).unwrap().title,
            "native name"
        );
    }
}

#[test]
fn path_matching_observes_component_boundaries() {
    assert!(path_is_inside_or_equal(
        Path::new("/workspace/repo/nested"),
        Path::new("/workspace/repo")
    ));
    assert!(!path_is_inside_or_equal(
        Path::new("/workspace/repo-other"),
        Path::new("/workspace/repo")
    ));
    assert!(path_is_inside_or_equal(
        Path::new("/workspace/repo/a/../nested"),
        Path::new("/workspace/repo")
    ));
}

#[test]
fn resolves_agent_home_overrides_with_traecli_precedence() {
    let process_home = Path::new("/home/tester");
    let roots = SessionRoots::from_values(
        process_home,
        Some(PathBuf::from("/config/claude")),
        Some(PathBuf::from("/config/codex")),
        Some(PathBuf::from("/config/traecli")),
        Some(PathBuf::from("/config/trae")),
    );
    assert_eq!(roots.claude, Path::new("/config/claude"));
    assert_eq!(roots.codex, Path::new("/config/codex"));
    assert_eq!(roots.traecli, Path::new("/config/traecli"));

    let roots = SessionRoots::from_values(
        process_home,
        None,
        None,
        None,
        Some(PathBuf::from("/config/trae")),
    );
    assert_eq!(roots.claude, process_home.join(".claude"));
    assert_eq!(roots.codex, process_home.join(".codex"));
    assert_eq!(roots.traecli, Path::new("/config/trae/cli"));

    let roots = SessionRoots::from_values(process_home, None, None, None, None);
    assert_eq!(roots.traecli, process_home.join(".trae/cli"));
}

#[test]
fn claude_provider_lists_by_workspace_and_finds_by_session_id() {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("repo");
    let other_worktree = directory.path().join("other-repo");
    fs::create_dir_all(&worktree).unwrap();
    let roots = roots(&directory);
    write(
        &roots.claude.join("projects/repo/claude-1.jsonl"),
        &format!(
            "{{\"type\":\"user\",\"sessionId\":\"claude-1\",\"timestamp\":\"2026-09-01T00:00:00Z\",\"cwd\":{},\"message\":{{\"content\":\"Claude prompt\"}}}}\n{{\"type\":\"ai-title\",\"aiTitle\":\"Claude title\"}}\n",
            serde_json::to_string(&worktree).unwrap()
        ),
    );
    write(
        &roots.claude.join("projects/repo/subagents/hidden.jsonl"),
        &format!(
            "{{\"type\":\"user\",\"sessionId\":\"hidden\",\"timestamp\":\"2026-09-01T00:00:00Z\",\"cwd\":{},\"message\":{{\"content\":\"Hidden\"}}}}\n",
            serde_json::to_string(&worktree).unwrap()
        ),
    );
    write(
        &roots.claude.join("projects/other/claude-2.jsonl"),
        &format!(
            "{{\"type\":\"user\",\"sessionId\":\"claude-2\",\"timestamp\":\"2026-09-01T00:00:00Z\",\"cwd\":{},\"message\":{{\"content\":\"Other prompt\"}}}}\n",
            serde_json::to_string(&other_worktree).unwrap()
        ),
    );

    let sessions = list_sessions(&worktree, Some("claude"), roots.clone());
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Claude title");
    assert_eq!(sessions[0].session_id, "claude-1");
    let found = find_session("claude", "claude-2", roots).unwrap();
    assert_eq!(found.cwd, other_worktree.to_string_lossy());
}

#[test]
fn sqlite_timestamps_support_seconds_and_milliseconds() {
    assert_eq!(sqlite_timestamp(1_725_100_000).timestamp(), 1_725_100_000);
    assert_eq!(
        sqlite_timestamp(1_725_100_123_000).timestamp_millis(),
        1_725_100_123_000
    );
}

#[test]
fn claude_uses_the_latest_recorded_cwd() {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("repo");
    fs::create_dir_all(&worktree).unwrap();
    let transcript = directory.path().join("claude.jsonl");
    write(
        &transcript,
        &format!(
            "{{\"type\":\"user\",\"sessionId\":\"claude-moved\",\"timestamp\":\"2026-09-01T00:00:00Z\",\"cwd\":\"/workspace/old\",\"message\":{{\"content\":\"Initial prompt\"}}}}\n{{\"type\":\"assistant\",\"timestamp\":\"2026-09-01T00:00:01Z\",\"cwd\":{},\"message\":{{\"content\":\"Moved\"}}}}\n",
            serde_json::to_string(&worktree).unwrap()
        ),
    );

    let session = parse_claude(&transcript, directory.path(), &worktree).unwrap();
    assert_eq!(session.session_id, "claude-moved");
    assert_eq!(session.title, "Initial prompt");
    assert_eq!(session.cwd, worktree.to_string_lossy());
}

#[test]
fn enum_dispatch_keeps_codex_and_traecli_histories_and_identities_separate() {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("repo");
    let roots = roots(&directory);
    for (agent, home) in [
        (Agent::Codex, &roots.codex),
        (Agent::TraeCli, &roots.traecli),
    ] {
        let rollout = home.join("sessions/shared-id.jsonl");
        write(
            &rollout,
            &format!(
                "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"{} prompt\"}}}}\n",
                agent.id()
            ),
        );
        let connection = create_state_db(home);
        insert_thread(
            &connection,
            "shared-id",
            &rollout,
            &worktree,
            "",
            1_725_100_100_000,
            false,
            "prompt",
            Some("user"),
            None,
            None,
            None,
        );
    }
    for agent in [Agent::Codex, Agent::TraeCli] {
        let provider = agent.sessions().unwrap();
        let sessions = provider.list_sessions(&roots, &worktree);
        assert_eq!(sessions.len(), 1);
        let found = provider.find_session(&roots, "shared-id").unwrap();
        assert_eq!(found, sessions[0]);
        let json = serde_json::to_value(&found).unwrap();
        assert_eq!(json["id"], format!("{}:shared-id", agent.id()));
        assert_eq!(json["agent"], agent.id());
        assert!(found.title.starts_with(agent.definition().display_name));
        assert!(json.get("transcript_path").is_none());
        assert!(json.get("trusted_root").is_none());
        let snapshot = serde_json::to_value(snapshot::read(found.locator()).unwrap()).unwrap();
        assert_eq!(snapshot["agent"], agent.id());
        assert_eq!(
            snapshot["turns"][0]["user"]["text"],
            format!("{} prompt", agent.id())
        );
    }
    assert_eq!(list_sessions(&worktree, None, roots).len(), 2);
    assert!(Agent::Gemini.sessions().is_none());
}

#[test]
fn current_titles_follow_native_renames_in_the_bound_codex_like_store() {
    let directory = tempfile::tempdir().unwrap();
    let roots = roots(&directory);
    let worktree = directory.path().join("repo");
    for (agent, home) in [
        (Agent::Codex, &roots.codex),
        (Agent::TraeCli, &roots.traecli),
    ] {
        let rollout = home.join("sessions/shared-id.jsonl");
        write(&rollout, "");
        let db = create_state_db(home);
        insert_thread(
            &db,
            "shared-id",
            &rollout,
            &worktree,
            "Original",
            1_725_100_100_000,
            false,
            "prompt",
            Some("user"),
            None,
            None,
            None,
        );
        let provider = agent.sessions().unwrap();
        let locator = provider
            .find_session(&roots, "shared-id")
            .unwrap()
            .locator();
        db.execute("UPDATE threads SET title = 'Generated rename'", [])
            .unwrap();
        assert_eq!(
            provider.current_title(&locator).as_deref(),
            Some("Generated rename")
        );

        // Older stores keep visible names in an append-only index. The newest
        // entry for this exact ID wins over SQLite's first-message title.
        write(
            &home.join("session_index.jsonl"),
            "{\"id\":\"shared-id\",\"thread_name\":\"Old index name\"}\n\
             {\"id\":\"shared-id\",\"thread_name\":\"New index name\"}\n\
             {\"id\":\"other-id\",\"thread_name\":\"Unrelated name\"}\n",
        );
        assert_eq!(
            provider.current_title(&locator).as_deref(),
            Some("New index name")
        );

        // Newer stores have a separate name column, with precedence over title.
        db.execute_batch("ALTER TABLE threads ADD COLUMN name TEXT;")
            .unwrap();
        let name = format!("{} current name", agent.id());
        db.execute("UPDATE threads SET name = ?1", [&name]).unwrap();
        assert_eq!(provider.current_title(&locator), Some(name));
        assert_eq!(locator.title, "Original");

        db.execute(
            "UPDATE threads SET rollout_path = '/another/session.jsonl'",
            [],
        )
        .unwrap();
        assert_eq!(provider.current_title(&locator), None);
        db.execute("DELETE FROM threads", []).unwrap();
        assert_eq!(provider.current_title(&locator), None);
    }
}

#[test]
fn current_claude_title_reads_later_renames_from_the_bound_transcript() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let roots = roots(&directory);
    let path = roots.claude.join("projects/repo/session.jsonl");
    write(
        &path,
        "{\"type\":\"user\",\"sessionId\":\"session\",\"cwd\":\"/workspace/repo\",\"message\":{\"content\":\"Original\"}}\n",
    );
    let provider = Agent::Claude.sessions().unwrap();
    let locator = provider.find_session(&roots, "session").unwrap().locator();
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    for (record, expected) in [
        (
            serde_json::json!({"type":"ai-title","sessionId":"session","aiTitle":"Generated rename"}),
            "Generated rename",
        ),
        (
            serde_json::json!({"type":"custom-title","sessionId":"session","customTitle":"Custom rename"}),
            "Custom rename",
        ),
        (
            serde_json::json!({"type":"ai-title","sessionId":"session","aiTitle":"Later generated name"}),
            "Custom rename",
        ),
    ] {
        writeln!(file, "{record}").unwrap();
        assert_eq!(provider.current_title(&locator).as_deref(), Some(expected));
    }
    let mut other = locator.clone();
    other.session_id = "another-session".into();
    assert_eq!(provider.current_title(&other), None);
    assert_eq!(
        Agent::Codex.sessions().unwrap().current_title(&locator),
        None
    );
    fs::remove_file(path).unwrap();
    assert_eq!(provider.current_title(&locator), None);
}
