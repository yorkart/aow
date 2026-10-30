use super::{TaskDb, TaskRecord};
fn task(id: &str, parent: Option<&str>) -> TaskRecord {
    TaskRecord {
        workspace_mode: "existing".into(),
        slug: None,
        id: id.into(),
        project_id: "project".into(),
        parent_id: parent.map(str::to_owned),
        prompt: "Review changes".into(),
        agent_id: "agent".into(),
        workspace: "/repo".into(),
        thread_id: format!("thread-{id}"),
        pinned: false,
        archived: false,
    }
}
#[test]
fn persists_hierarchy_pins_and_session_ownership_across_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite3");
    let database = TaskDb::open(Some(&path)).unwrap();
    database.insert(&task("parent", None)).unwrap();
    database.insert(&task("child", Some("parent"))).unwrap();
    database.pin("child", true).unwrap();
    drop(database);
    let rows = TaskDb::open(Some(&path)).unwrap().list().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].parent_id.as_deref(), Some("parent"));
    assert_eq!(rows[1].thread_id, "thread-child");
    assert!(rows[1].pinned);
}
#[test]
fn rejects_missing_cross_project_and_archived_parents() {
    let database = TaskDb::open(None).unwrap();
    assert!(database.insert(&task("orphan", Some("missing"))).is_err());
    database.insert(&task("parent", None)).unwrap();
    let mut child = task("child", Some("parent"));
    child.project_id = "other".into();
    assert!(database.insert(&child).is_err());
    database.finish("parent", true).unwrap();
    assert!(database.insert(&task("child", Some("parent"))).is_err());
    assert_eq!(database.list().unwrap().len(), 1);
}
#[test]
fn archive_and_delete_cover_entire_subtree_without_affecting_siblings() {
    let database = TaskDb::open(None).unwrap();
    for row in [
        task("parent", None),
        task("child", Some("parent")),
        task("grandchild", Some("child")),
        task("other", None),
    ] {
        database.insert(&row).unwrap();
    }
    database.pin("grandchild", true).unwrap();
    database.finish("parent", true).unwrap();
    let rows = database.list().unwrap();
    assert!(rows[..3].iter().all(|row| row.archived && !row.pinned));
    assert!(!rows[3].archived);
    assert!(database.pin("parent", true).is_err());
    database.finish("parent", false).unwrap();
    assert_eq!(database.list().unwrap(), vec![task("other", None)]);
}
#[test]
fn duplicate_thread_cannot_acquire_another_task_owner() {
    let database = TaskDb::open(None).unwrap();
    database.insert(&task("first", None)).unwrap();
    let mut second = task("second", None);
    second.thread_id = "thread-first".into();
    assert!(database.insert(&second).is_err());
    assert_eq!(database.list().unwrap().len(), 1);
}

#[test]
fn upgrades_existing_task_rows_without_losing_session_or_pin() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite3");
    let connection = crate::sqlite::open(Some(&path)).unwrap();
    connection
        .execute_batch(include_str!("sql/initialize.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(domain, version) VALUES ('AowTasks', 1)",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO tasks(id, project_id, prompt, agent_id, workspace, thread_id, pinned) VALUES ('old', 'project', 'Review', 'agent', '/repo', 'old-thread', 1)", []).unwrap();
    drop(connection);
    let database = TaskDb::open(Some(&path)).unwrap();
    let row = &database.list().unwrap()[0];
    assert_eq!(row.workspace_mode, "existing");
    assert_eq!(row.slug, None);
    assert_eq!(row.thread_id, "old-thread");
    assert!(row.pinned);
}
