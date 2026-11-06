use super::*;

fn task(id: &str, dependencies: &[&str]) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        project_id: "project".into(),
        status: TaskStatus::Todo,
        created_at: Some(1_700_000_000_000),
        updated_at: Some(1_700_000_000_000),
        completed_at: None,
        prompt: format!("Task {id}"),
        agent_id: "agent".into(),
        agent_role_id: None,
        is_orchestrator: false,
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

#[test]
fn business_status_preserves_json_and_sqlite_values() {
    let db = TaskDb::open(None).unwrap();
    db.insert(&task("status", &[])).unwrap();
    for (status, value) in [
        (TaskStatus::Todo, "todo"),
        (TaskStatus::InProgress, "in_progress"),
        (TaskStatus::Backlog, "backlog"),
        (TaskStatus::InReview, "in_review"),
        (TaskStatus::Done, "done"),
        (TaskStatus::Blocked, "blocked"),
        (TaskStatus::Cancelled, "cancelled"),
    ] {
        let json = serde_json::json!(value);
        assert_eq!(serde_json::to_value(status).unwrap(), json);
        let decoded: TaskStatus = serde_json::from_value(json).unwrap();
        db.set_status("status", decoded).unwrap();
        let stored: String = db
            .lock()
            .unwrap()
            .query_row("SELECT status FROM tasks WHERE id = 'status'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, value);
        assert_eq!(db.status("status").unwrap(), Some(status));
        assert_eq!(db.list().unwrap()[0].status, status);
    }
}

#[test]
fn invalid_business_status_is_rejected_at_json_and_sqlite_boundaries() {
    let db = TaskDb::open(None).unwrap();
    db.insert(&task("invalid", &[])).unwrap();
    for value in [
        "",
        "waiting",
        "running",
        "result_available",
        "Done",
        "in_progess",
    ] {
        assert!(serde_json::from_value::<TaskStatus>(serde_json::json!(value)).is_err());
        assert!(
            db.lock()
                .unwrap()
                .query_row("SELECT ?1", [value], |row| row.get::<_, TaskStatus>(0))
                .is_err()
        );
        assert!(
            db.lock()
                .unwrap()
                .execute("UPDATE tasks SET status = ?1 WHERE id = 'invalid'", [value])
                .is_err()
        );
    }
}

#[test]
fn task_timestamps_persist_and_completion_tracks_business_status() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let db = TaskDb::open(Some(&path)).unwrap();
    let original = task("lifecycle", &[]);
    db.insert(&original).unwrap();
    db.update("lifecycle", Some("Revised goal"), None, None, None)
        .unwrap();
    let edited = db.list().unwrap().remove(0);
    assert_eq!(edited.created_at, original.created_at);
    assert!(edited.updated_at > original.updated_at);
    assert_eq!(edited.completed_at, None);

    db.set_status("lifecycle", TaskStatus::Done).unwrap();
    let completed = db.list().unwrap().remove(0);
    assert!(completed.completed_at.is_some());
    assert_eq!(completed.completed_at, completed.updated_at);
    assert_eq!(completed.created_at, original.created_at);
    // Retrying a status mutation must not invent a new completion time.
    db.set_status("lifecycle", TaskStatus::Done).unwrap();
    assert_eq!(db.list().unwrap()[0], completed);
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.list().unwrap()[0], completed);

    db.set_status("lifecycle", TaskStatus::InProgress).unwrap();
    assert_eq!(db.list().unwrap()[0].completed_at, None);
    db.set_status("lifecycle", TaskStatus::Cancelled).unwrap();
    assert_eq!(db.list().unwrap()[0].completed_at, None);
    db.set_status("lifecycle", TaskStatus::Backlog).unwrap();
    let reopened = db.list().unwrap().remove(0);
    db.enqueue_message(&message("more-work", "lifecycle"))
        .unwrap();
    db.update_message("more-work", DeliveryState::Delivered, None, 0)
        .unwrap();
    assert_eq!(
        db.list().unwrap()[0],
        reopened,
        "Conversation delivery must not change task progress or timestamps"
    );
}

fn message(id: &str, task: &str) -> MessageDelivery {
    MessageDelivery {
        id: id.into(),
        request_id: id.into(),
        task_id: task.into(),
        thread_id: format!("thread-{task}"),
        prompt: "Investigate".into(),
        source: None,
        event_id: None,
        state: DeliveryState::Pending,
        created_at: 1,
        attempts: 0,
        retry_at: 0,
        error: None,
    }
}
fn event(id: &str) -> TaskEvent {
    TaskEvent {
        id: id.into(),
        request_id: id.into(),
        project_id: "project".into(),
        task_id: "worker".into(),
        thread_id: "thread-worker".into(),
        kind: EventKind::Progress,
        message: "POC confirmed; design still needs work".into(),
        target: Some(EventTarget {
            task_id: "parent".into(),
            thread_id: "thread-parent".into(),
        }),
        source: None,
        created_at: 2,
    }
}
#[test]
fn explicit_reports_are_durable_idempotent_and_independent_of_task_status() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let db = TaskDb::open(Some(&path)).unwrap();
    db.insert(&task("parent", &[])).unwrap();
    db.insert_with_message(&task("worker", &[]), Some(&message("initial", "worker")))
        .unwrap();
    db.update_message("initial", DeliveryState::Delivered, None, 0)
        .unwrap();
    assert!(
        db.inbox(None, false, false, 0, 30)
            .unwrap()
            .items
            .is_empty(),
        "Admission is not a report"
    );
    db.set_status("worker", TaskStatus::Blocked).unwrap();
    let report = db.publish_event(&event("poc")).unwrap();
    let mut retry = report.clone();
    retry.id = "another-id".into();
    retry.created_at = 90;
    assert_eq!(db.publish_event(&retry).unwrap(), report);
    retry.message = "Different result".into();
    assert!(db.publish_event(&retry).is_err());
    db.update_inbox("poc", Some(true), Some(true)).unwrap();
    assert_eq!(
        db.pending_messages().unwrap().len(),
        1,
        "Reading and archiving do not suppress delivery"
    );
    assert_eq!(db.status("worker").unwrap(), Some(TaskStatus::Blocked));
    let id = db.pending_messages().unwrap()[0].id.clone();
    db.update_message(
        &id,
        DeliveryState::Blocked,
        Some("Receiver offline".into()),
        0,
    )
    .unwrap();
    db.retry_delivery("poc").unwrap();
    assert_eq!(db.pending_messages().unwrap()[0].id, id);
    db.update_message(&id, DeliveryState::Delivered, None, 0)
        .unwrap();
    db.update_message(&id, DeliveryState::Pending, Some("late failure".into()), 5)
        .unwrap();
    assert!(db.retry_delivery("poc").is_err());
    db.remove("worker").unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    let inbox = db.inbox(None, false, true, 0, 30).unwrap();
    assert_eq!(inbox.items[0].event, report);
    assert!(inbox.items[0].read);
    assert_eq!(
        inbox.items[0].delivery.as_ref().unwrap().state,
        DeliveryState::Delivered
    );
}

#[test]
fn queued_messages_survive_restart_and_reject_request_id_reuse() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let db = TaskDb::open(Some(&path)).unwrap();
    db.insert(&task("worker", &[])).unwrap();
    let first = message("request", "worker");
    db.enqueue_message(&first).unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.pending_messages().unwrap(), vec![first.clone()]);
    assert_eq!(
        db.message_for_prompt("worker", "thread-worker", &first.id)
            .unwrap(),
        Some(first.clone())
    );
    assert!(
        db.message_for_prompt("other-task", "thread-worker", &first.id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.message_for_prompt("worker", "other-thread", &first.id)
            .unwrap()
            .is_none()
    );
    let mut retry = first.clone();
    retry.id = "new-id".into();
    assert_eq!(db.enqueue_message(&retry).unwrap(), first);
    retry.prompt = "Different request".into();
    assert!(db.enqueue_message(&retry).is_err());
}

#[test]
fn migration_preserves_notifications_pending_messages_and_conversation_policies() {
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    {
        let mut connection = aow_sqlite::open(Some(&path)).unwrap();
        aow_sqlite::migrate(
            &mut connection,
            "AowTasks",
            &[
                include_str!("sql/initialize.sql"),
                include_str!("sql/roles.sql"),
                include_str!("sql/routing.sql"),
                include_str!("sql/collaboration.sql"),
                include_str!("sql/executions.sql"),
            ],
        )
        .unwrap();
        connection.execute("INSERT INTO tasks (id,project_id,prompt,agent_id,workspace,workspace_mode,status) VALUES ('worker','project','Goal','agent','/repo','existing','waiting')",[]).unwrap();
        connection.execute("INSERT INTO tasks (id,project_id,prompt,agent_id,workspace,workspace_mode,status) VALUES ('parent','project','Parent','agent','/repo','existing','todo')",[]).unwrap();
        connection
            .execute(
                "UPDATE tasks SET source_task_id = 'parent' WHERE id = 'worker'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_dependencies VALUES ('worker', 'parent')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_resources VALUES ('worker', 'acp', 'thread-worker')",
                [],
            )
            .unwrap();
        for (id, state) in [("complete", "result_available"), ("pending", "queued")] {
            let payload = json!({"id":id,"request_id":id,"task_id":"worker","project_id":"project","thread_id":"thread-worker","prompt":"Original task","state":state,"result":"POC verified","created_at":1,
                "source":{"task_id":"parent","thread_id":"thread-parent","turn_id":"user-turn","message_id":"user-message","request":"Original user request","parent_run_id":"old-parent"},
                "execution":{"workspace":"/repo","workspace_intent":"analyze","target_task_id":null,"target_revision":null}});
            connection.execute("INSERT INTO task_runs (id,task_id,thread_id,parent_run_id,state,payload,request_id) VALUES (?1,'worker','thread-worker','old-parent',?2,?3,?1)",rusqlite::params![id,state,payload.to_string()]).unwrap();
        }
        connection
            .execute(
                "INSERT INTO task_inbox VALUES ('legacy-event','complete','project',2,1,1)",
                [],
            )
            .unwrap();
        connection.execute("INSERT INTO task_wakeups VALUES ('legacy-event','legacy-prompt','old-parent','pending',?1)",[json!({"attempts":1,"retry_at":0,"error":null}).to_string()]).unwrap();
    }
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.status("worker").unwrap(), Some(TaskStatus::Blocked));
    let worker = db
        .list()
        .unwrap()
        .into_iter()
        .find(|task| task.id == "worker")
        .unwrap();
    assert_eq!(worker.source_task_id.as_deref(), Some("parent"));
    assert_eq!(worker.dependencies, vec!["parent"]);
    assert_eq!(
        worker.resources,
        vec![TaskResource {
            kind: "acp".into(),
            id: "thread-worker".into()
        }]
    );
    assert_eq!(
        (worker.created_at, worker.updated_at, worker.completed_at),
        (None, None, None)
    );
    let inbox = db.inbox(None, false, true, 0, 30).unwrap();
    assert_eq!(inbox.items[0].event.message, "POC verified");
    assert!(inbox.items[0].read);
    assert_eq!(
        inbox.items[0].event.target.as_ref().unwrap().thread_id,
        "thread-parent"
    );
    assert_eq!(
        inbox.items[0].delivery.as_ref().unwrap().id,
        "legacy-prompt",
        "ACP receipt identity must survive migration"
    );
    assert_eq!(db.pending_messages().unwrap().len(), 2);
    assert_eq!(
        db.conversation_policy("thread-worker")
            .unwrap()
            .unwrap()
            .workspace_intent
            .as_deref(),
        Some("analyze")
    );
    let remaining:u32=db.lock().unwrap().query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('task_runs','task_wakeups','task_inbox')",[],|row|row.get(0)).unwrap();
    assert_eq!(remaining, 0);
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.pending_messages().unwrap().len(), 2);
}

#[test]
fn orchestrator_migration_preserves_identity_relationships_and_pending_reports() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut report = event("legacy-report");
    report.target = Some(EventTarget {
        task_id: "router-project".into(),
        thread_id: "thread-router".into(),
    });
    report.source = Some(MessageSource {
        task_id: "router-project".into(),
        thread_id: "thread-router".into(),
        turn_id: Some("original-turn".into()),
        message_id: Some("original-message".into()),
        request: "Ask the Router to investigate".into(),
    });
    let mut delivery = message("inbox-legacy-report", "router-project");
    delivery.thread_id = "thread-router".into();
    delivery.event_id = Some(report.id.clone());
    delivery.source = report.source.clone();
    {
        let mut connection = aow_sqlite::open(Some(&path)).unwrap();
        aow_sqlite::migrate(
            &mut connection,
            "AowTasks",
            &[
                include_str!("sql/initialize.sql"),
                include_str!("sql/roles.sql"),
                include_str!("sql/routing.sql"),
                include_str!("sql/collaboration.sql"),
                include_str!("sql/executions.sql"),
                include_str!("sql/messages.sql"),
                include_str!("sql/properties.sql"),
            ],
        )
        .unwrap();
        connection.execute_batch("INSERT INTO tasks (id,project_id,prompt,agent_id,workspace,workspace_mode,is_router,initial_thread_id,created_at,updated_at) VALUES
            ('router-project','project','Router','agent','/repo','existing',1,'thread-router',10,20),
            ('worker','project','Router','agent','/repo','existing',0,'thread-worker',10,20),
            ('custom','other','My coordinator','agent','/other','existing',1,'thread-custom',10,20);
            UPDATE tasks SET source_task_id='router-project', source_thread_id='thread-router' WHERE id='worker';
            INSERT INTO task_resources VALUES ('router-project','acp','thread-router');
            INSERT INTO task_dependencies VALUES ('worker','router-project');").unwrap();
        connection.execute("INSERT INTO task_events (id,project_id,thread_id,request_id,created_at,payload,is_read) VALUES (?1,?2,?3,?4,?5,?6,1)",
            rusqlite::params![report.id,report.project_id,report.thread_id,report.request_id,report.created_at,serde_json::to_string(&report).unwrap()]).unwrap();
        super::messages::insert(&connection, &delivery).unwrap();
    }
    let db = TaskDb::open(Some(&path)).unwrap();
    let tasks = db.list().unwrap();
    let orchestrator = tasks
        .iter()
        .find(|task| task.id == "router-project")
        .unwrap();
    assert!(orchestrator.is_orchestrator);
    assert_eq!(orchestrator.prompt, "Orchestrator");
    assert_eq!(
        orchestrator.initial_thread_id.as_deref(),
        Some("thread-router")
    );
    assert_eq!(orchestrator.resources[0].id, "thread-router");
    assert_eq!(
        (orchestrator.created_at, orchestrator.updated_at),
        (Some(10), Some(20))
    );
    let worker = tasks.iter().find(|task| task.id == "worker").unwrap();
    assert!(!worker.is_orchestrator);
    assert_eq!(
        worker.prompt, "Router",
        "ordinary user task text is preserved"
    );
    assert_eq!(worker.source_task_id.as_deref(), Some("router-project"));
    assert_eq!(worker.source_thread_id.as_deref(), Some("thread-router"));
    assert_eq!(worker.dependencies, ["router-project"]);
    assert_eq!(
        tasks
            .iter()
            .find(|task| task.id == "custom")
            .unwrap()
            .prompt,
        "My coordinator"
    );
    let inbox = db.inbox(None, false, false, 0, 30).unwrap();
    assert_eq!(inbox.items[0].event, report);
    assert!(inbox.items[0].read);
    assert_eq!(inbox.items[0].delivery.as_ref(), Some(&delivery));
    assert_eq!(
        db.message_for_prompt("router-project", "thread-router", &delivery.id)
            .unwrap(),
        Some(delivery.clone())
    );
    let mut duplicate = task("duplicate", &[]);
    duplicate.is_orchestrator = true;
    assert!(db.insert(&duplicate).is_err());
    let connection = db.lock().unwrap();
    let old_columns: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name='is_router'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_columns, 0);
    let index_count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='tasks_orchestrator_project'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 1);
    drop(connection);
    drop(db);
    let reopened = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(reopened.list().unwrap(), tasks);
    assert_eq!(reopened.pending_messages().unwrap(), vec![delivery]);
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
fn orchestrator_uniqueness_and_delegation_provenance_survive_restart_and_source_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite3");
    let db = TaskDb::open(Some(&path)).unwrap();
    let mut orchestrator = task("orchestrator", &[]);
    orchestrator.is_orchestrator = true;
    db.insert(&orchestrator).unwrap();
    let mut duplicate = task("other-orchestrator", &[]);
    duplicate.is_orchestrator = true;
    assert!(db.insert(&duplicate).is_err());
    let mut child = task("child", &[]);
    child.source_task_id = Some(orchestrator.id.clone());
    child.source_thread_id = Some("thread-orchestrator".into());
    child.workspace_intent = Some("review".into());
    child.target_task_id = Some("implementation".into());
    child.target_revision = Some("revision".into());
    db.insert(&child).unwrap();
    drop(db);
    let db = TaskDb::open(Some(&path)).unwrap();
    assert_eq!(db.list().unwrap(), vec![orchestrator, child.clone()]);
    db.remove("orchestrator").unwrap();
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
