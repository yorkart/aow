use super::registry;
use super::registry::{Identity, Registry, SessionKey, Target};
use crate::sessions::SessionEnvironment;
use std::collections::HashSet;

use crate::sessions::AgentSessionLocator;
use aow_host_contract::terminal::TerminalAgentProcess;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::{Duration, SystemTime},
};

fn identity(title: &str) -> Identity {
    Identity {
        agent: "codex".into(),
        process: Some(TerminalAgentProcess {
            pid: 123,
            start_time: "1000".into(),
            pi_binding: None,
            cwd: "/workspace/demo".into(),
        }),
        cwd: "/workspace/demo".into(),
        source_root: "/history".into(),
        target: Target::Title(title.into()),
    }
}

fn fixture(root: &Path, id: &str, modified: u64) -> AgentSessionLocator {
    let path = root.join(format!("{id}.jsonl"));
    let file = File::create(&path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(modified))
        .unwrap();
    AgentSessionLocator {
        agent: "codex",
        session_id: id.into(),
        title: "Same title".into(),
        cwd: root.into(),
        transcript_path: path,
        trusted_root: root.into(),
    }
}

fn stop(locator: &AgentSessionLocator) {
    writeln!(
        OpenOptions::new()
            .append(true)
            .open(&locator.transcript_path)
            .unwrap(),
        "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"one\",\"last_agent_message\":\"本轮完整结论\"}}}}"
    )
    .unwrap();
}

#[test]
fn completion_notifications_forward_only_their_readers_observed_usage() {
    let root = tempfile::tempdir().unwrap();
    let locator = fixture(root.path(), "usage", 1);
    let mut registry = Registry::default();
    registry.register("pane".into(), identity("title"), vec![locator.clone()]);
    writeln!(
        OpenOptions::new()
            .append(true)
            .open(&locator.transcript_path)
            .unwrap(),
        "{}",
        serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{
            "last_token_usage":{"input_tokens":100,"output_tokens":20},
            "total_token_usage":{"input_tokens":10000,"output_tokens":2000}
        }}})
    )
    .unwrap();
    stop(&locator);
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].usage.unwrap().total_tokens, 120);
    assert_eq!(events[0].session_id, "usage");
    stop(&locator);
    assert_eq!(registry.poll()[0].usage, None);
}

#[test]
fn refresh_keeps_existing_positions_and_new_candidates_start_at_eof() {
    let root = tempfile::tempdir().unwrap();
    let mut a = fixture(root.path(), "a", 1);
    let b = fixture(root.path(), "b", 2);
    let mut registry = Registry::default();
    registry.register("pane".into(), identity("title"), vec![a.clone()]);
    stop(&a);
    stop(&b);
    a.title = "Renamed while running".into();
    registry.register("pane".into(), identity("title"), vec![a.clone(), b.clone()]);
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "a");
    assert_eq!(events[0].title, "Renamed while running");
    stop(&b);
    assert_eq!(registry.poll()[0].session_id, "b");
    registry.register("pane".into(), identity("other"), vec![b.clone()]);
    stop(&a);
    registry.register("pane".into(), identity("title"), vec![a]);
    assert!(registry.poll().is_empty()); // re-entry has no historical cursor
}

#[test]
fn codex_candidate_refresh_preserves_unread_completions_across_database_failure() {
    use crate::sessions::SessionRoots;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let sessions = root.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let locator = fixture(&sessions, "session", 1);
    let db = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("sql/tests/create_threads.sql"))
        .unwrap();
    db.execute(
        include_str!("sql/tests/insert_thread.sql"),
        rusqlite::params![
            locator.session_id,
            locator.transcript_path.to_str(),
            locator.cwd.to_str()
        ],
    )
    .unwrap();
    let mut identity = identity("Task");
    identity.target = Target::Id(locator.session_id.clone());
    identity.cwd = locator.cwd.to_string_lossy().into_owned();
    identity.source_root = root.clone();
    let roots = SessionRoots::from_configuration(
        &root,
        &SessionEnvironment::from([("CODEX_HOME".into(), root.clone())]),
    );
    let mut registry = Registry::default();
    registry.refresh(
        "pane".into(),
        identity.clone(),
        registry::candidates(&identity, roots.clone()),
    );
    assert_eq!(registry.readers.len(), 1);

    db.execute_batch(include_str!("sql/tests/lock_database.sql"))
        .unwrap();
    let unavailable = registry::candidates(&identity, roots.clone());
    assert!(unavailable.is_none());
    registry.refresh("pane".into(), identity.clone(), unavailable);
    stop(&locator);
    assert_eq!(registry.readers.len(), 1);
    db.execute_batch(include_str!("sql/tests/unlock_database.sql"))
        .unwrap();
    registry.refresh(
        "pane".into(),
        identity.clone(),
        registry::candidates(&identity, roots.clone()),
    );
    let events = registry.poll();
    assert_eq!(
        events.len(),
        1,
        "recovery must not reattach at EOF and skip this completion"
    );
    assert_eq!(events[0].instance_ids, ["pane"]);
    assert!(registry.poll().is_empty());

    db.execute(include_str!("sql/tests/delete_threads.sql"), [])
        .unwrap();
    registry.refresh(
        "pane".into(),
        identity.clone(),
        registry::candidates(&identity, roots),
    );
    assert!(registry.readers.is_empty());
    assert!(registry.bindings.is_empty());
}

#[test]
fn concrete_codex_ids_keep_same_titles_separate_and_follow_rollout_replacement() {
    let root = tempfile::tempdir().unwrap();
    let a = fixture(root.path(), "a", 1);
    let b = fixture(root.path(), "b", 2);
    let mut first = identity("Same title");
    first.target = Target::Id("a".into());
    let mut second = identity("Same title");
    second.target = Target::Id("b".into());
    second.process.as_mut().unwrap().pid = 456;
    let mut registry = Registry::default();
    registry.register("pane-a".into(), first.clone(), vec![a.clone()]);
    registry.register("pane-b".into(), second, vec![b.clone()]);
    assert!(registry.same_candidates("pane-a", std::slice::from_ref(&a)));
    stop(&a);
    stop(&b);
    let events = registry.poll();
    assert_eq!(events.len(), 2);
    for event in events {
        assert_eq!(event.instance_ids, [format!("pane-{}", event.session_id)]);
    }

    let mut replacement = fixture(root.path(), "a-reverted", 3);
    replacement.session_id = "a".into();
    stop(&replacement); // Reverted history must not replay on attachment.
    assert!(registry.unchanged("pane-a", &first));
    assert!(!registry.same_candidates("pane-a", std::slice::from_ref(&replacement)));
    registry.register("pane-a".into(), first, vec![replacement.clone()]);
    assert!(!registry.readers.contains_key(&SessionKey::from(&a)));
    assert_eq!(registry.readers.len(), 2);
    assert!(registry.poll().is_empty());
    stop(&a); // Old rollout no longer belongs to this pane.
    stop(&replacement);
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "a");
    assert_eq!(events[0].instance_ids, ["pane-a"]);

    registry.readers.clear(); // Unchanged paths must preserve budget eviction.
    assert!(registry.same_candidates("pane-a", &[replacement]));
}

#[test]
fn same_title_sessions_are_independent_but_shared_sessions_have_one_reader() {
    let root = tempfile::tempdir().unwrap();
    let a = fixture(root.path(), "a", 1);
    let b = fixture(root.path(), "b", 2);
    let mut registry = Registry::default();
    registry.register(
        "pane-1".into(),
        identity("title"),
        vec![a.clone(), b.clone()],
    );
    registry.register(
        "pane-2".into(),
        identity("another matching title"),
        vec![a.clone()],
    );
    assert_eq!(registry.readers.len(), 2);
    stop(&a);
    stop(&b);
    let events = registry.poll();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .find(|event| event.session_id == "a")
            .unwrap()
            .instance_ids,
        ["pane-1", "pane-2"]
    );
    registry.unregister("pane-1");
    assert_eq!(registry.readers.len(), 1);
    stop(&a);
    stop(&b);
    assert_eq!(registry.poll()[0].instance_ids, ["pane-2"]);
    registry.retain_instances(&HashSet::new());
    assert!(registry.readers.is_empty());
    assert!(registry.poll().is_empty());
}

#[test]
fn registration_is_capped_at_five_and_unchanged_binding_does_not_refill() {
    let root = tempfile::tempdir().unwrap();
    let candidates: Vec<_> = (0..7)
        .map(|i| fixture(root.path(), &i.to_string(), i))
        .collect();
    let mut registry = Registry::default();
    registry.register("pane".into(), identity("title"), candidates);
    assert_eq!(registry.readers.len(), 5);
    assert!(registry.unchanged("pane", &identity("title")));
    registry.readers.clear(); // budget eviction leaves registration fixed
    assert!(registry.unchanged("pane", &identity("title")));
    let mut restarted = identity("title");
    restarted.process.as_mut().unwrap().start_time = "2000".into();
    assert!(!registry.unchanged("pane", &restarted));
}

#[test]
fn global_budget_evicts_least_recently_modified_rollout_not_oldest_registration() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = Registry::default();
    let mut candidates = Vec::new();
    for i in 0..50 {
        let locator = fixture(root.path(), &format!("s{i}"), 100 + i);
        registry.register(
            format!("p{i}"),
            identity(&format!("title-{i}")),
            vec![locator.clone()],
        );
        candidates.push(locator);
    }
    // The oldest registration has fresh output and must survive eviction.
    stop(&candidates[0]);
    let extra = fixture(root.path(), "extra", 200);
    registry.register("extra".into(), identity("extra"), vec![extra]);
    assert_eq!(registry.readers.len(), 50);
    assert!(
        registry
            .readers
            .contains_key(&SessionKey::from(&candidates[0]))
    );
    assert!(
        !registry
            .readers
            .contains_key(&SessionKey::from(&candidates[1]))
    );
    assert!(registry.unchanged("p1", &identity("title-1")));
}

#[test]
fn new_registration_for_same_title_refreshes_all_owners() {
    let root = tempfile::tempdir().unwrap();
    let candidates: Vec<_> = (0..6)
        .map(|i| fixture(root.path(), &i.to_string(), i))
        .collect();
    let mut registry = Registry::default();
    registry.register("pane-1".into(), identity("title"), candidates[..5].to_vec());
    stop(&candidates[1]);
    stop(&candidates[5]); // before it joins the refreshed candidate set
    registry.register("pane-2".into(), identity("title"), candidates[1..].to_vec());
    assert_eq!(registry.readers.len(), 5);
    assert!(
        !registry
            .readers
            .contains_key(&SessionKey::from(&candidates[0]))
    );
    assert_eq!(
        registry.bindings["pane-1"].sessions,
        registry.bindings["pane-2"].sessions
    );
    let events = registry.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "1");
    assert_eq!(events[0].instance_ids, ["pane-1", "pane-2"]);
}

fn event() -> super::Completion {
    super::Completion {
        agent_display_name: "Codex".into(),
        agent: "codex".into(),
        session_id: "native".into(),
        title: "Node title".into(),
        cwd: "/remote-only/worktree".into(),
        turn_id: Some("turn".into()),
        conclusion: Some("complete".into()),
        usage: None,
        instance_ids: vec!["pane".into()],
    }
}
#[test]
fn durable_log_replays_only_after_cursor_and_rejects_expired_or_future_cursors() {
    use super::storage;
    let root = tempfile::tempdir().unwrap();
    let mut db = storage::open(root.path()).unwrap();
    assert_eq!(storage::append(&mut db, &[event(), event()]).unwrap(), 2);
    assert_eq!(storage::read(&db, 0).unwrap().len(), 2);
    assert_eq!(storage::read(&db, 1).unwrap()[0]["sequence"], 2);
    assert!(storage::read(&db, 2).unwrap().is_empty());
    drop(db);
    let mut db = storage::open(root.path()).unwrap();
    assert_eq!(storage::bounds(&db).unwrap(), (1, 2));
    assert_eq!(storage::append(&mut db, &[event()]).unwrap(), 3);
    assert_eq!(storage::read(&db, 2).unwrap()[0]["sequence"], 3);
    assert!(storage::read(&db, 4).is_err());
    storage::append(&mut db, &vec![event(); 4096]).unwrap();
    assert!(storage::read(&db, 2).is_err());
    assert_eq!(storage::bounds(&db).unwrap(), (4, 4099));
    assert_eq!(storage::read(&db, 3).unwrap()[0]["sequence"], 4);
}
