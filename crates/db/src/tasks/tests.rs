use super::*;

fn task(id: &str, dependencies: &[&str]) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        project_id: "project".into(),
        status: "todo".into(),
        prompt: format!("Task {id}"),
        agent_id: "agent".into(),
        agent_role_id: None,
        is_router: false,
        source_task_id: None,
        source_thread_id: None,
        workspace_intent: None,
        target_task_id: None,
        target_revision: None,
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

fn execution(id: &str, task_id: &str, parent: Option<&str>) -> TaskRun {
    TaskRun {
        id: id.into(),
        request_id: id.into(),
        task_id: task_id.into(),
        project_id: "project".into(),
        thread_id: format!("thread-{task_id}"),
        prompt: format!("Work {id}"),
        source: Some(RunSource {
            task_id: "source".into(),
            thread_id: "thread-source".into(),
            turn_id: Some("source-turn".into()),
            message_id: Some("source-message".into()),
            request: "Original wording".into(),
            parent_run_id: parent.map(str::to_owned),
        }),
        execution: None,
        trigger: "message".into(),
        caused_by_run_id: None,
        state: "queued".into(),
        result: None,
        error: None,
        created_at: 1,
        finished_at: None,
        attempts: 0,
        retry_at: 0,
    }
}

#[test]
fn results_and_deliveries_are_durable_independent_and_immutable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let db = TaskDb::open(Some(&path)).unwrap();
    let first = execution("first", "worker", None);
    db.insert_with_run(&task("worker", &[]), Some(&first))
        .unwrap();
    assert!(
        db.insert_with_run(&task("rollback", &[]), Some(&first))
            .is_err()
    );
    assert_eq!(db.list().unwrap().len(), 1, "task/run insertion is atomic");
    db.finish_run(
        "first",
        "result_available",
        Some("First result".into()),
        None,
        2,
    )
    .unwrap();
    db.finish_run("first", "failed", None, Some("Late duplicate".into()), 3)
        .unwrap();
    let mut second = execution("second", "worker", None);
    second.source.as_mut().unwrap().thread_id = "different-requester".into();
    db.enqueue_run(&second).unwrap();
    assert_eq!(db.enqueue_run(&second).unwrap(), second);
    let mut retried = second.clone();
    retried.id = "new-local-attempt".into();
    assert_eq!(db.enqueue_run(&retried).unwrap().id, second.id);
    second.prompt = "Different payload".into();
    assert!(db.enqueue_run(&second).is_err());
    db.finish_run("second", "failed", None, Some("No access".into()), 4)
        .unwrap();
    db.update_inbox("result-first", Some(true), Some(true))
        .unwrap();
    db.remove("worker").unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(
        db.run("first").unwrap().unwrap().result.as_deref(),
        Some("First result")
    );
    let page = db.inbox(None, false, false, 0, 30).unwrap();
    assert_eq!(page.unread, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].run.id, "second");
    assert_eq!(
        page.items[0].delivery.as_ref().unwrap().thread_id,
        "different-requester"
    );
    assert_eq!(
        db.active_deliveries().unwrap().len(),
        2,
        "read/archive cannot consume returns"
    );
    assert_eq!(
        db.inbox(None, false, true, 0, 30).unwrap().items[0].run.id,
        "first"
    );
    assert!(
        db.process_delivery("result-first", Some("Premature".into()), 5)
            .is_err()
    );
    db.enqueue_wakeup(
        "result-first",
        &execution("wakeup-result-first", "source", None),
    )
    .unwrap();
    db.process_delivery("result-first", Some("Summary".into()), 5)
        .unwrap();
    db.process_delivery("result-first", Some("Duplicate".into()), 6)
        .unwrap();
    assert_eq!(db.active_deliveries().unwrap().len(), 1);
    assert!(db.retry_delivery("result-first", "retry").is_err());
    db.update_delivery(
        "result-second",
        "blocked",
        Some("Source unavailable".into()),
        0,
    )
    .unwrap();
    db.retry_delivery("result-second", "retry-second").unwrap();
    assert_eq!(db.active_deliveries().unwrap()[0].prompt_id, "retry-second");
    assert_eq!(
        db.run("second").unwrap().unwrap().state,
        "failed",
        "retrying a return never reruns work"
    );
}

#[test]
fn wakeups_have_independent_results_and_preserve_causal_and_return_addresses() {
    let db = TaskDb::open(None).unwrap();
    db.insert(&task("worker", &[])).unwrap();
    db.enqueue_run(&execution("parent", "worker", None))
        .unwrap();
    let mut child = execution("child", "helper", Some("parent"));
    child.source.as_mut().unwrap().task_id = "worker".into();
    child.source.as_mut().unwrap().thread_id = "thread-worker".into();
    db.enqueue_run(&child).unwrap();
    let followup = db
        .enqueue_run(&execution("followup", "worker", Some("child")))
        .unwrap();
    assert_eq!(
        followup.state, "queued",
        "an active ancestor receives a later run without a waiting dependency"
    );
    db.finish_run(
        "parent",
        "result_available",
        Some("Delegated".into()),
        None,
        1,
    )
    .unwrap();
    let frozen = db.run("parent").unwrap().unwrap();
    assert!(frozen.terminal());
    assert_eq!(db.list().unwrap()[0].status, "in_progress");
    db.set_status("worker", "waiting").unwrap();
    db.finish_run(
        "child",
        "result_available",
        Some("Findings".into()),
        None,
        2,
    )
    .unwrap();
    let mut request = execution("wakeup-result-child", "worker", None);
    request.thread_id = "thread-worker".into();
    let wakeup = db.enqueue_wakeup("result-child", &request).unwrap();
    assert_eq!(db.enqueue_wakeup("result-child", &request).unwrap(), wakeup);
    assert_eq!(wakeup.trigger, "wakeup");
    assert_eq!(wakeup.caused_by_run_id.as_deref(), Some("child"));
    assert_eq!(wakeup.source, frozen.source);
    assert_eq!(
        db.run_for_prompt(&wakeup.id).unwrap(),
        Some(wakeup.id.clone())
    );
    assert!(
        !db.active_runs()
            .unwrap()
            .iter()
            .any(|run| run.id == wakeup.id)
    );
    db.process_delivery("result-child", Some("Combined conclusion".into()), 3)
        .unwrap();
    db.process_delivery("result-child", Some("Duplicate".into()), 4)
        .unwrap();
    assert_eq!(db.run("parent").unwrap().unwrap(), frozen);
    assert_eq!(
        db.run(&wakeup.id).unwrap().unwrap().result.as_deref(),
        Some("Combined conclusion")
    );
    let upstream = db
        .active_deliveries()
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.run_id == wakeup.id)
        .unwrap();
    assert_eq!(upstream.thread_id, "thread-source");
    assert_eq!(
        db.list().unwrap()[0].status,
        "waiting",
        "run outcomes do not change business acceptance"
    );
    db.set_status("worker", "done").unwrap();
    assert_eq!(db.list().unwrap()[0].status, "done");
    assert!(db.set_status("worker", "result_available").is_err());
}

#[test]
fn upgrading_waiting_runs_freezes_old_results_and_keeps_pending_returns() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    {
        let mut connection = crate::sqlite::open(Some(&path)).unwrap();
        crate::sqlite::migrate(
            &mut connection,
            "AowTasks",
            &[
                include_str!("sql/initialize.sql"),
                include_str!("sql/roles.sql"),
                include_str!("sql/routing.sql"),
                include_str!("sql/collaboration.sql"),
            ],
        )
        .unwrap();
        connection.execute("INSERT INTO tasks (id, project_id, prompt, agent_id, workspace, workspace_mode) VALUES ('worker', 'project', 'Goal', 'agent', '/repo', 'existing')", []).unwrap();
        let mut old = execution("old", "worker", None);
        old.state = "waiting".into();
        old.result = Some("Delegated; awaiting findings".into());
        let mut payload = serde_json::to_value(&old).unwrap();
        for key in ["execution", "trigger", "caused_by_run_id"] {
            payload.as_object_mut().unwrap().remove(key);
        }
        connection.execute("INSERT INTO task_runs VALUES ('old','worker','old','thread-worker',NULL,'waiting',?1)", [payload.to_string()]).unwrap();
        let mut child = execution("legacy-child", "helper", Some("old"));
        child.source.as_mut().unwrap().task_id = "worker".into();
        child.source.as_mut().unwrap().thread_id = "thread-worker".into();
        child.state = "result_available".into();
        child.result = Some("Child result".into());
        connection
            .execute(
                "INSERT INTO task_runs VALUES (?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![
                    child.id,
                    child.task_id,
                    child.request_id,
                    child.thread_id,
                    "old",
                    child.state,
                    serde_json::to_string(&child).unwrap()
                ],
            )
            .unwrap();
        super::collaboration::finish(&connection, &mut child, 2).unwrap();
    }
    let db = TaskDb::open(Some(&path)).unwrap();
    let run = db.run("old").unwrap().unwrap();
    assert_eq!(run.state, "result_available");
    assert_eq!(run.result.as_deref(), Some("Delegated; awaiting findings"));
    assert_eq!(run.execution.unwrap().workspace, "/repo");
    assert_eq!(db.list().unwrap()[0].status, "waiting");
    assert_eq!(db.active_deliveries().unwrap().len(), 2);
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.inbox(None, false, false, 0, 30).unwrap().items.len(), 2);
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
fn router_uniqueness_and_delegation_provenance_survive_restart_and_source_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite3");
    let db = TaskDb::open(Some(&path)).unwrap();
    let mut router = task("router", &[]);
    router.is_router = true;
    db.insert(&router).unwrap();
    let mut duplicate = task("other-router", &[]);
    duplicate.is_router = true;
    assert!(db.insert(&duplicate).is_err());
    let mut child = task("child", &[]);
    child.source_task_id = Some(router.id.clone());
    child.source_thread_id = Some("thread-router".into());
    child.workspace_intent = Some("review".into());
    child.target_task_id = Some("implementation".into());
    child.target_revision = Some("revision".into());
    db.insert(&child).unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.list().unwrap(), vec![router, child.clone()]);
    db.remove("router").unwrap();
    assert_eq!(db.list().unwrap(), vec![child]);
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
