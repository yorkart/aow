use super::*;

fn record(log: &AcpDebugLog, direction: &str, wire: Value) {
    log.record_line(direction, &wire.to_string());
}
fn new_request(log: &AcpDebugLog, thread_id: &str, id: Value) {
    log.attach(thread_id);
    log.store
        .state
        .lock()
        .unwrap()
        .connections
        .get_mut(&log.connection_id)
        .unwrap()
        .requests
        .insert(
            ("outgoing".into(), id.to_string()),
            RequestContext {
                thread_id: Some(thread_id.into()),
                method: "session/new".into(),
                ..Default::default()
            },
        );
    record(
        log,
        "outgoing",
        json!({"id":id,"method":"session/new","params":{"cwd":"/repo"}}),
    );
}

#[test]
fn interleaved_threads_keep_shared_initialization_and_correlate_both_rpc_directions() {
    let log = AcpDebugLog::default();
    record(&log, "outgoing", json!({"id":0,"method":"initialize"}));
    record(
        &log,
        "incoming",
        json!({"id":0,"result":{"protocolVersion":1}}),
    );
    new_request(&log, "a", json!(1));
    new_request(&log, "b", json!(2));
    // Reverse replies and notifications before the creation response.
    record(
        &log,
        "incoming",
        json!({"method":"session/update","params":{"sessionId":"remote-b","update":{}}}),
    );
    record(
        &log,
        "incoming",
        json!({"id":2,"result":{"sessionId":"remote-b"}}),
    );
    record(
        &log,
        "incoming",
        json!({"id":1,"result":{"sessionId":"remote-a"}}),
    );
    record(
        &log,
        "outgoing",
        json!({"id":3,"method":"session/prompt","params":{"sessionId":"remote-a"}}),
    );
    record(
        &log,
        "incoming",
        json!({"id":3,"method":"session/request_permission","params":{"sessionId":"remote-b"}}),
    );
    record(&log, "outgoing", json!({"id":3,"result":{"outcome":"b"}}));
    record(
        &log,
        "incoming",
        json!({"id":3,"result":{"stopReason":"a"}}),
    );
    log.record_line("stderr", "unknown process diagnostic");
    record(
        &log,
        "incoming",
        json!({"method":"session/update","params":{"sessionId":"unknown"}}),
    );
    let a = log.thread_messages("a");
    let b = log.thread_messages("b");
    assert_eq!(a.len(), 6);
    assert_eq!(b.len(), 7);
    for (id, entries) in [("a", a), ("b", b)] {
        assert_eq!(
            entries
                .iter()
                .filter(|line| line["scope"] == "connection")
                .count(),
            2
        );
        assert!(
            entries
                .iter()
                .all(|line| line["thread_id"] == id || line["scope"] == "connection")
        );
        assert!(
            entries
                .iter()
                .all(|line| line["connection_id"] == log.connection_id)
        );
    }
    assert_eq!(
        log.messages()
            .iter()
            .filter(|line| line["scope"] == "unattributed")
            .count(),
        2
    );
    assert!(log.thread_messages("unrelated").is_empty());
}

#[test]
fn failures_and_request_scoped_elicitation_work_before_session_id_exists() {
    let log = AcpDebugLog::default();
    new_request(&log, "thread", json!(7));
    record(
        &log,
        "incoming",
        json!({"id":"7","method":"elicitation/create","params":{"requestId":7}}),
    );
    record(
        &log,
        "outgoing",
        json!({"id":"7","result":{"action":"accept"}}),
    );
    record(
        &log,
        "incoming",
        json!({"id":7,"error":{"code":-32000,"message":"Authentication required"}}),
    );
    let lines = log.thread_messages("thread");
    assert_eq!(lines.len(), 4);
    assert!(
        lines
            .iter()
            .all(|line| line["thread_id"] == "thread" && line["session_id"].is_null())
    );
    assert!(
        log.store.state.lock().unwrap().connections[&log.connection_id]
            .requests
            .is_empty()
    );
}

#[test]
fn remote_ids_and_request_ids_are_isolated_between_connections() {
    let first = AcpDebugLog::default();
    let second = AcpDebugLog::default();
    for (log, id) in [(&first, "first"), (&second, "second")] {
        log.bind_session("same-remote", id);
        record(
            log,
            "outgoing",
            json!({"id":1,"method":"session/load","params":{"sessionId":"same-remote"}}),
        );
        record(log, "incoming", json!({"id":1,"result":{}}));
        assert_eq!(log.thread_messages(id).len(), 2);
    }
    assert!(first.thread_messages("second").is_empty());
    assert!(second.thread_messages("first").is_empty());
}

#[test]
fn buffers_and_messages_remain_bounded() {
    let log = AcpDebugLog::default();
    for _ in 0..501 {
        log.record_line("stderr", "diagnostic");
    }
    log.record_line("stderr", &"字".repeat(65537));
    let messages = log.messages();
    assert_eq!(messages.len(), 500);
    assert_eq!(
        messages.last().unwrap()["message"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        65536
    );
}

#[test]
fn cancellation_uses_the_initiating_direction_and_unknown_events_are_not_shared() {
    let log = AcpDebugLog::default();
    new_request(&log, "thread", json!(7));
    record(
        &log,
        "outgoing",
        json!({"method":"$/cancel_request","params":{"requestId":7}}),
    );
    record(
        &log,
        "incoming",
        json!({"id":999,"result":{"text":"unattributed"}}),
    );
    record(
        &log,
        "incoming",
        json!({"method":"_unknown","params":{"text":"unattributed"}}),
    );
    let messages = log.thread_messages("thread");
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().all(|entry| entry["thread_id"] == "thread"));
    assert_eq!(log.messages().len(), 4);
}

#[test]
fn url_completions_follow_session_request_and_shared_auth_scopes_after_responses() {
    let log = AcpDebugLog::default();
    log.bind_session("remote-a", "a");
    log.bind_session("remote-b", "b");
    new_request(&log, "pending", json!(7));
    record(&log, "outgoing", json!({"id":8,"method":"authenticate"}));
    for (id, scope) in [
        ("a", json!({"sessionId":"remote-a"})),
        ("b", json!({"sessionId":"remote-b"})),
        ("pending", json!({"requestId":7})),
        ("auth", json!({"requestId":8})),
    ] {
        let mut params = json!({"mode":"url","elicitationId":id,"url":"https://example.test/login","message":"Log in"});
        params
            .as_object_mut()
            .unwrap()
            .extend(scope.as_object().unwrap().clone());
        record(
            &log,
            "incoming",
            json!({"id":id,"method":"elicitation/create","params":params}),
        );
        record(
            &log,
            "outgoing",
            json!({"id":id,"result":{"action":"accept"}}),
        );
    }
    record(
        &log,
        "incoming",
        json!({"id":7,"error":{"code":-32000,"message":"Login required"}}),
    );
    record(&log, "incoming", json!({"id":8,"result":{}}));
    // Completion has neither sessionId nor requestId and arrives after both replies.
    for id in ["auth", "b", "pending", "a", "unknown"] {
        record(
            &log,
            "incoming",
            json!({"method":"elicitation/complete","params":{"elicitationId":id}}),
        );
    }
    for thread in ["a", "b", "pending"] {
        let completions: Vec<_> = log
            .thread_messages(thread)
            .into_iter()
            .filter(|entry| {
                entry["message"]
                    .as_str()
                    .unwrap()
                    .contains("elicitation/complete")
            })
            .collect();
        assert_eq!(
            completions.len(),
            2,
            "own completion and shared authentication only"
        );
        assert!(completions.iter().any(|entry| entry["thread_id"] == thread));
        assert!(
            completions
                .iter()
                .any(|entry| entry["scope"] == "connection")
        );
    }
    assert_eq!(log.messages().last().unwrap()["scope"], "unattributed");
    assert!(
        log.store.state.lock().unwrap().connections[&log.connection_id]
            .elicitations
            .is_empty()
    );
}

#[test]
fn url_completions_bind_replayed_sessions_and_stay_connection_local() {
    for thread in ["a", "b"] {
        let log = AcpDebugLog::default();
        record(
            &log,
            "incoming",
            json!({"id":1,"method":"elicitation/create","params":{"mode":"url","sessionId":"same-session","elicitationId":"same-id"}}),
        );
        log.bind_session("same-session", thread);
        record(
            &log,
            "outgoing",
            json!({"id":1,"result":{"action":"accept"}}),
        );
        record(
            &log,
            "incoming",
            json!({"method":"elicitation/complete","params":{"elicitationId":"same-id"}}),
        );
        assert_eq!(log.thread_messages(thread).len(), 3);
        assert!(
            log.messages()
                .iter()
                .all(|entry| entry["thread_id"] == thread)
        );
    }
}

#[test]
fn abandoned_request_and_url_contexts_are_bounded_independently_of_messages() {
    let log = AcpDebugLog::default();
    log.bind_session("remote", "thread");
    for id in 0..1100 {
        record(
            &log,
            "incoming",
            json!({"id":id,"method":"elicitation/create","params":{"mode":"url","sessionId":"remote","elicitationId":id.to_string()}}),
        );
    }
    {
        let store = log.store.state.lock().unwrap();
        let state = &store.connections[&log.connection_id];
        assert_eq!(state.messages.len(), 500);
        assert_eq!(state.requests.len(), 1024);
        assert_eq!(state.elicitations.len(), 1024);
    }
    for id in ["0", "1099"] {
        record(
            &log,
            "incoming",
            json!({"method":"elicitation/complete","params":{"elicitationId":id}}),
        );
        assert_eq!(
            log.messages().last().unwrap()["scope"],
            if id == "0" { "unattributed" } else { "thread" }
        );
    }
    log.archive();
    let store = log.store.state.lock().unwrap();
    let state = &store.connections[&log.connection_id];
    assert!(state.requests.is_empty() && state.elicitations.is_empty());
}

#[test]
fn archived_connections_are_evicted_even_when_old_handles_remain_alive() {
    let store = AcpDebugLogStore::default();
    let active = store.create_log();
    active.bind_session("remote", "thread");
    let mut archived = Vec::new();
    for _ in 0..=store::MAX_ARCHIVED_CONNECTIONS {
        let log = store.create_log();
        log.attach("thread");
        record(&log, "outgoing", json!({"id":1,"method":"initialize"}));
        log.archive();
        log.archive(); // Failure handling and process exit can both archive a connection.
        archived.push(log);
    }
    assert_eq!(
        store.state.lock().unwrap().connections.len(),
        store::MAX_ARCHIVED_CONNECTIONS + 1
    );
    assert!(store.messages(&archived[0].connection_id).is_none());
    archived[0].record_line("stderr", "late output must not resurrect an evicted log");
    assert!(store.messages(&archived[0].connection_id).is_none());
    record(
        &active,
        "incoming",
        json!({"method":"session/update","params":{"sessionId":"remote"}}),
    );
    assert_eq!(
        store.messages(&active.connection_id).unwrap()[0]["thread_id"],
        "thread"
    );
}

#[test]
fn global_byte_budget_evicts_old_messages_without_losing_live_correlation() {
    let store = AcpDebugLogStore::default();
    let first = store.create_log();
    first.bind_session("remote", "thread");
    first.record_line("stderr", "oldest");
    let payload = "x".repeat(65_000);
    for _ in 0..3 {
        let log = store.create_log();
        for _ in 0..60 {
            log.record_line("stderr", &payload);
        }
    }
    {
        let state = store.state.lock().unwrap();
        let bytes: usize = state.connections.values().map(|log| log.bytes).sum();
        assert!(bytes <= store::MAX_BYTES);
        assert!(bytes > store::MAX_BYTES - 70_000);
        assert_eq!(
            state.connections.len(),
            4,
            "active contexts survive eviction"
        );
    }
    assert!(store.messages(&first.connection_id).unwrap().is_empty());
    record(
        &first,
        "incoming",
        json!({"method":"session/update","params":{"sessionId":"remote"}}),
    );
    assert_eq!(store.thread_messages("thread")[0]["thread_id"], "thread");
}

#[test]
fn global_record_budget_bounds_many_small_messages() {
    let store = AcpDebugLogStore::default();
    let logs: Vec<_> = (0..21).map(|_| store.create_log()).collect();
    for (index, log) in logs.iter().enumerate() {
        for _ in 0..500 {
            log.record_line("stderr", &index.to_string());
        }
    }
    let state = store.state.lock().unwrap();
    assert_eq!(
        state
            .connections
            .values()
            .map(|log| log.messages.len())
            .sum::<usize>(),
        store::MAX_MESSAGES
    );
    assert!(
        state.connections[&logs[0].connection_id]
            .messages
            .is_empty()
    );
    assert_eq!(
        state.connections[&logs[20].connection_id].messages.len(),
        500
    );
}

#[test]
fn deletion_purges_private_logs_and_contexts_but_preserves_shared_connections() {
    let store = AcpDebugLogStore::default();
    let shared = store.create_log();
    shared.bind_session("remote-a", "a");
    shared.bind_session("remote-b", "b");
    record(&shared, "outgoing", json!({"id":0,"method":"initialize"}));
    for (id, session) in [("a", "remote-a"), ("b", "remote-b")] {
        record(
            &shared,
            "incoming",
            json!({"id":id,"method":"elicitation/create","params":{"mode":"url","sessionId":session,"elicitationId":id}}),
        );
    }
    let failed = store.create_log();
    failed.attach("a");
    failed.record_line("stderr", "failed initialization");
    failed.archive();
    store.remove_thread("a");
    assert!(store.messages(&failed.connection_id).is_none());
    assert!(store.thread_messages("a").is_empty());
    assert_eq!(
        store.thread_messages("b").len(),
        2,
        "shared initialize and B's request"
    );
    {
        let state = store.state.lock().unwrap();
        let shared = &state.connections[&shared.connection_id];
        assert_eq!(shared.sessions.len(), 1);
        assert_eq!(
            shared.requests.len(),
            2,
            "shared initialize and B's request"
        );
        assert_eq!(shared.elicitations.len(), 1);
    }
    record(
        &shared,
        "incoming",
        json!({"method":"elicitation/complete","params":{"elicitationId":"b"}}),
    );
    assert_eq!(store.thread_messages("b").last().unwrap()["thread_id"], "b");
    store.remove_thread("b");
    assert!(store.messages(&shared.connection_id).unwrap().is_empty());
    shared.bind_session("remote-c", "c");
    record(
        &shared,
        "incoming",
        json!({"method":"session/update","params":{"sessionId":"remote-c"}}),
    );
    assert_eq!(
        store.thread_messages("c").len(),
        1,
        "reused connection still records new sessions"
    );
}

#[test]
fn evicted_request_retains_its_response_method() {
    let log = AcpDebugLog::default();
    log.bind_session("remote", "thread");
    record(
        &log,
        "outgoing",
        json!({"id":42,"method":"session/load","params":{"sessionId":"remote"}}),
    );
    for _ in 0..501 {
        log.record_line("stderr", "diagnostic");
    }
    record(&log, "incoming", json!({"id":42,"result":{}}));
    let messages = log.thread_messages("thread");
    let response = messages.last().unwrap();
    assert_eq!(response["request_method"], "session/load");
    assert_eq!(response["session_id"], "remote");
    assert_eq!(response["thread_id"], "thread");
}
