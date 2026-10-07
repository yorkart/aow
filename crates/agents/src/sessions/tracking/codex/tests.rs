use super::*;
use aow_process::{OpenFile, ProcessCommand};

const A: &str = "01a1135c-7ea2-7f62-8f53-93061095d838";
const B: &str = "01a1135c-b691-7a52-b85a-8b262c004426";

fn file(path: PathBuf, writable: bool) -> OpenFile {
    OpenFile { path, writable }
}

fn writer(root: &Path, id: &str) -> OpenFile {
    file(
        root.join("thread-writer-locks").join(format!("{id}.lock")),
        true,
    )
}

fn resolved(id: &str) -> SessionResolution {
    SessionResolution::Resolved(SessionTarget::Id(id.into()))
}

struct Store {
    _directory: tempfile::TempDir,
    root: PathBuf,
    cwd: PathBuf,
    db: rusqlite::Connection,
}

impl Store {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().canonicalize().unwrap();
        std::fs::create_dir(home.join("sessions")).unwrap();
        std::fs::create_dir(home.join("thread-writer-locks")).unwrap();
        let db = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT,
            cwd TEXT, title TEXT, name TEXT, created_at INTEGER, updated_at INTEGER);",
        )
        .unwrap();
        Self {
            _directory: directory,
            cwd: home.clone(),
            root: SessionRoots::from_configuration(
                &home,
                &SessionEnvironment::from([("CODEX_HOME".into(), home.clone())]),
            )
            .codex,
            db,
        }
    }

    fn insert(&self, id: &str, title: &str) {
        self.db
            .execute(
                "INSERT INTO threads VALUES (?1, ?2, ?3, ?4, NULL, 1, 1)",
                rusqlite::params![
                    id,
                    self.root
                        .join("sessions")
                        .join(format!("rollout-2026-10-07T06-36-31-{id}.jsonl"))
                        .to_string_lossy(),
                    self.cwd.to_string_lossy(),
                    title
                ],
            )
            .unwrap();
    }
}

#[test]
fn ownership_separates_same_title_sessions_and_follows_switches() {
    let store = Store::new();
    store.insert(A, "Same title");
    store.insert(B, "Same title");
    let a = writer(&store.root, A);
    let b = writer(&store.root, B);
    assert_eq!(
        resolve_files(
            &store.root,
            &store.cwd,
            "Same title",
            std::slice::from_ref(&a)
        ),
        resolved(A)
    );
    assert_eq!(
        resolve_files(
            &store.root,
            &store.cwd,
            "Same title",
            std::slice::from_ref(&b)
        ),
        resolved(B)
    );
    assert_eq!(
        resolve_files(
            &store.root,
            &store.cwd,
            "Same title",
            &[a.clone(), b.clone()]
        ),
        SessionResolution::NotFound
    );
    store
        .db
        .execute("UPDATE threads SET name='New task' WHERE id=?1", [B])
        .unwrap();
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "New task", &[a.clone(), b.clone()]),
        resolved(B)
    );
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "", &[a.clone(), b]),
        SessionResolution::NotFound
    );
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "", std::slice::from_ref(&a)),
        resolved(A)
    );
    assert_eq!(
        resolve_files(
            &store.root,
            &store.cwd,
            "Different title",
            std::slice::from_ref(&a)
        ),
        SessionResolution::NotFound
    );
    assert_eq!(
        resolve_files(&store.root, Path::new("/different-cwd"), "Same title", &[a]),
        SessionResolution::NotFound
    );
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "Same title", &[]),
        SessionResolution::NotFound
    );
}

#[test]
fn database_failures_are_unavailable_but_missing_sessions_are_not_found() {
    let store = Store::new();
    store.insert(A, "Task");
    let open = [writer(&store.root, A)];
    let resolve = || resolve_files(&store.root, &store.cwd, "Task", &open);
    let candidates = || {
        TrackingAgent::Codex.try_candidate_sessions(
            &SessionTarget::Id(A.into()),
            &store.cwd,
            SessionRoots::from_configuration(
                &store.cwd,
                &SessionEnvironment::from([("CODEX_HOME".into(), store.root.clone())]),
            ),
        )
    };
    assert_eq!(resolve(), resolved(A));
    assert_eq!(candidates().unwrap().len(), 1);
    store.db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert_eq!(resolve(), SessionResolution::Unavailable);
    assert!(candidates().is_none());
    store.db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(resolve(), resolved(A));
    assert_eq!(candidates().unwrap().len(), 1);

    let path = store.root.join("state_5.sqlite");
    let unavailable = store.root.join("unavailable.sqlite");
    std::fs::rename(&path, &unavailable).unwrap();
    assert_eq!(resolve(), SessionResolution::Unavailable);
    assert!(candidates().is_none());
    std::fs::rename(&unavailable, &path).unwrap();
    store.db.execute("DELETE FROM threads", []).unwrap();
    assert_eq!(resolve(), SessionResolution::NotFound);
    assert!(candidates().unwrap().is_empty());
}

#[test]
fn unpersisted_new_thread_prevents_binding_the_old_rollout() {
    let store = Store::new();
    store.insert(A, "Old task");
    let old = file(
        store
            .root
            .join("sessions")
            .join(format!("rollout-2026-10-07T06-36-31-{A}.jsonl")),
        true,
    );
    let open = [old, writer(&store.root, A), writer(&store.root, B)];
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "Old task", &open),
        SessionResolution::NotFound
    );
    store.insert(B, "New task");
    assert_eq!(
        resolve_files(&store.root, &store.cwd, "New task", &open),
        resolved(B)
    );
}

#[test]
fn ignores_readers_other_homes_and_coordination_and_preserves_reverted_thread_id() {
    let store = Store::new();
    let open = [
        file(
            store
                .root
                .join("sessions")
                .join(format!("rollout-2026-10-07T06-36-31-{A}_{B}.jsonl")),
            true,
        ),
        file(writer(&store.root, B).path, false),
        writer(Path::new("/another-home"), B),
        writer(&store.root, ".coordination"),
        writer(&store.root, "invalid"),
    ];
    assert_eq!(
        files::thread_ids(&store.root, &open),
        BTreeSet::from([A.into()])
    );
}

#[test]
#[cfg(unix)]
fn redirected_sessions_directory_keeps_writer_locks_in_the_configured_home() {
    let store = Store::new();
    let redirected = store.cwd.join("redirected");
    let sessions = store.root.join("sessions");
    std::fs::rename(&sessions, &redirected).unwrap();
    std::os::unix::fs::symlink(&redirected, &sessions).unwrap();
    let open = [
        writer(&store.root, A),
        file(
            redirected.join(format!("rollout-2026-10-07T06-36-31-{B}.jsonl")),
            true,
        ),
    ];
    assert_eq!(
        files::thread_ids(&store.root, &open),
        BTreeSet::from([A.into(), B.into()])
    );
}

#[test]
fn actual_argv_must_describe_a_native_isolated_codex_tui() {
    let command = |executable: &str, args: &[&str]| ProcessCommand {
        executable: Some(executable.into()),
        arguments: args.iter().flat_map(|arg| arg.bytes().chain([0])).collect(),
    };
    assert!(isolated_tui(&command(
        "/opt/codex",
        &["codex", "--no-daemon"]
    )));
    for args in [
        vec!["codex"],
        vec!["codex", "--", "--no-daemon"],
        vec!["codex", "--no-daemon", "--remote=unix:///tmp/server"],
        vec!["codex", "-c", "--no-daemon"],
        vec!["codex", "exec", "--no-daemon"],
    ] {
        assert!(!isolated_tui(&command("/opt/codex", &args)), "{args:?}");
    }
    assert!(!isolated_tui(&command(
        "/bin/sh",
        &["sh", "-c", "codex --no-daemon"]
    )));
    assert!(!isolated_tui(&command(
        "/usr/bin/node",
        &["node", "/opt/@openai/codex/bin/codex.js", "--no-daemon"]
    )));
    assert!(!isolated_tui(&ProcessCommand::default()));
}

#[tokio::test]
async fn unmanaged_process_uses_existing_title_resolution_without_inspection() {
    assert_eq!(
        Codex
            .resolve_live_session(LiveSessionContext {
                pid: Some(-1),
                cwd: "/project",
                title: "Existing title",
                environment: &Default::default(),
            })
            .await,
        SessionResolution::Resolved(SessionTarget::Title("Existing title".into()))
    );
}
