use super::*;

fn task(id: &str, dependencies: &[&str]) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        project_id: "project".into(),
        prompt: format!("Task {id}"),
        agent_id: "agent".into(),
        workspace: "/repo".into(),
        workspace_mode: "existing".into(),
        slug: None,
        initial_thread_id: Some(format!("thread-{id}")),
        pinned: false,
        archived: false,
        dependencies: dependencies.iter().map(|id| (*id).into()).collect(),
        resources: vec![TaskResource {
            kind: "acp".into(),
            id: format!("thread-{id}"),
        }],
    }
}

#[test]
fn dependencies_are_a_dag_and_failed_edits_are_atomic() {
    let db = TaskDb::open(None).unwrap();
    db.insert(&task("a", &[])).unwrap();
    db.insert(&task("b", &["a"])).unwrap();
    db.insert(&task("c", &["a", "b"])).unwrap();
    let before = db.list().unwrap();
    for dependencies in [vec!["a".into()], vec!["c".into()], vec!["missing".into()]] {
        assert!(
            db.update("a", Some("changed"), Some(true), None, Some(&dependencies))
                .is_err()
        );
        assert_eq!(db.list().unwrap(), before);
    }
    let mut other = task("other", &[]);
    other.project_id = "elsewhere".into();
    db.insert(&other).unwrap();
    assert!(
        db.update("a", None, None, None, Some(&["other".into()]))
            .is_err()
    );
    assert!(db.insert(&task("duplicate", &["a", "a"])).is_err());
    assert!(!db.list().unwrap().iter().any(|task| task.id == "duplicate"));
}

#[test]
fn task_resources_survive_reopening_and_do_not_mix_shared_worktrees() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite3");
    let db = TaskDb::open(Some(&path)).unwrap();
    db.insert(&task("a", &[])).unwrap();
    let mut isolated = task("b", &["a"]);
    isolated.workspace_mode = "new_worktree".into();
    isolated.slug = Some("review-ui".into());
    db.insert(&isolated).unwrap();
    let helper = TaskResource {
        kind: "acp".into(),
        id: "helper".into(),
    };
    let terminal = TaskResource {
        kind: "terminal".into(),
        id: "shell".into(),
    };
    db.attach("a", &helper).unwrap();
    db.attach("a", &terminal).unwrap();
    db.attach("a", &helper).unwrap();
    assert!(db.attach("b", &helper).is_err());
    let before = db.list().unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.list().unwrap(), before);
    db.update("a", None, None, Some(true), None).unwrap();
    let archived = db.list().unwrap();
    assert!(archived[0].archived);
    assert!(!archived[1].archived);
    assert_eq!(archived[0].resources.len(), 3);
    assert!(db.attach("a", &helper).is_err());
    db.update("a", None, None, Some(false), None).unwrap();
    db.remove("a").unwrap();
    let remaining = db.list().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "b");
    assert!(remaining[0].dependencies.is_empty());
    assert_eq!(remaining[0].resources.len(), 1);
}
