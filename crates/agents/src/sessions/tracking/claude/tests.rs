use super::*;

#[test]
fn interrupted_or_failed_reply_is_not_a_completion_conclusion() {
    for error in [
        serde_json::json!({"type":"assistant", "isApiErrorMessage":true, "message":{"content":"API failure"}}),
        serde_json::json!({"type":"assistant", "interruptedMessageId":"reply", "message":{"content":"Interrupted"}}),
    ] {
        let mut parser = Parser::default();
        parser.consume("s", &serde_json::json!({"type":"assistant", "message":{"id":"reply", "content":"Earlier reply"}}));
        parser.consume("s", &error);
        let event = parser
            .consume(
                "s",
                &serde_json::json!({"type":"system", "subtype":"turn_duration"}),
            )
            .unwrap();
        assert!(event.conclusion.is_none());
    }
}

#[cfg(unix)]
fn cli_fixture(root: &Path, response: &str) -> SessionEnvironment {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join("bin");
    let config = root.join("config");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(bin.join("claude"), "#!/bin/sh\n[ \"$1\" = agents ] && [ \"$2\" = --json ] || exit 9\nprintf x >> \"$CLAUDE_CONFIG_DIR/queries\"\ncat \"$CLAUDE_CONFIG_DIR/active.json\"\n").unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(config.join("active.json"), response).unwrap();
    [
        ("HOME".into(), root.into()),
        (
            "PATH".into(),
            format!("{}:/usr/bin:/bin", bin.display()).into(),
        ),
        ("CLAUDE_CONFIG_DIR".into(), config),
    ]
    .into_iter()
    .collect()
}

#[tokio::test]
async fn lookup_cache_is_scoped_to_environment_and_preserves_failures() {
    let mut cache = ClaudeSessionCache::default();
    let environment: SessionEnvironment = [("CLAUDE_CONFIG_DIR".into(), "/first".into())]
        .into_iter()
        .collect();
    let sessions = cache
        .sessions(&environment, async {
            Some(vec![ClaudeSession {
                pid: Some(123),
                session_id: Some("Session-A".into()),
            }])
        })
        .await
        .unwrap();
    assert_eq!(
        claude_session_id(&sessions, 123).as_deref(),
        Some("Session-A")
    );
    // Cache hits must not poll the query, including a different PID's lookup.
    let cached = cache
        .sessions(&environment, async {
            panic!("a cached environment must not query again")
        })
        .await
        .unwrap();
    assert_eq!(
        claude_session_id(&cached, 123).as_deref(),
        Some("Session-A")
    );
    assert_eq!(claude_session_id(&cached, 456), None);

    let unavailable: SessionEnvironment = [("CLAUDE_CONFIG_DIR".into(), "/second".into())]
        .into_iter()
        .collect();
    assert!(cache.sessions(&unavailable, async { None }).await.is_none());
    assert!(
        cache
            .sessions(&unavailable, async {
                panic!("a cached failure must not query again")
            })
            .await
            .is_none()
    );

    let empty: SessionEnvironment = [("CLAUDE_CONFIG_DIR".into(), "/third".into())]
        .into_iter()
        .collect();
    assert!(
        cache
            .sessions(&empty, async { Some(Vec::new()) })
            .await
            .unwrap()
            .is_empty()
    );

    // Expire entries explicitly so cache lifetimes need no sleeps or subprocesses.
    for entry in &mut cache.entries {
        entry.captured = Instant::now() - Duration::from_secs(6);
    }
    assert!(
        cache
            .sessions(&environment, async { Some(Vec::new()) })
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        cache
            .sessions(&unavailable, async {
                panic!("failures have a longer lifetime than successful queries")
            })
            .await
            .is_none()
    );
    cache
        .entries
        .iter_mut()
        .find(|entry| entry.environment == unavailable)
        .unwrap()
        .captured = Instant::now() - Duration::from_secs(31);
    assert!(
        cache
            .sessions(&unavailable, async { Some(Vec::new()) })
            .await
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_lookup_reads_each_environment_and_distinguishes_invalid_and_empty_json() {
    // Allow process startup on loaded hosts without changing the production
    // three-second deadline. Its enforcement is tested separately below.
    let timeout = Duration::from_secs(15);
    let first = tempfile::tempdir().unwrap();
    let id = "Session-A";
    let environment = cli_fixture(
        first.path(),
        &format!(r#"[{{"pid":123,"id":"short-job","sessionId":"{id}"}}]"#),
    );
    let sessions = query_claude(&environment, timeout).await.unwrap();
    assert_eq!(claude_session_id(&sessions, 123).as_deref(), Some(id));
    assert_eq!(
        std::fs::read_to_string(first.path().join("config/queries")).unwrap(),
        "x"
    );

    let second = tempfile::tempdir().unwrap();
    let unavailable = cli_fixture(second.path(), "invalid JSON");
    assert!(query_claude(&unavailable, timeout).await.is_none());
    assert_eq!(
        std::fs::read_to_string(second.path().join("config/queries")).unwrap(),
        "x"
    );

    let third = tempfile::tempdir().unwrap();
    let empty = cli_fixture(third.path(), "[]");
    assert!(query_claude(&empty, timeout).await.unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn native_lookup_enforces_its_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let environment = cli_fixture(directory.path(), "[]");
    // exec keeps the delayed command in the child that kill_on_drop owns.
    std::fs::write(
        directory.path().join("bin/claude"),
        "#!/bin/sh\nexec /bin/sleep 30\n",
    )
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        TrackingAgent::Claude.resolve_live_session_with_timeout(
            LiveSessionContext {
                pid: Some(123),
                cwd: directory.path().to_str().unwrap(),
                title: "",
                environment: &environment,
            },
            Duration::from_millis(20),
        ),
    )
    .await
    .expect("the query did not enforce its deadline");
    assert_eq!(result, SessionResolution::Unavailable);
}

#[test]
fn claude_uses_opaque_session_ids_for_the_exact_pid_and_rejects_ambiguity() {
    let id = "550e8400-e29b-41d4-a716-446655440000";
    let sessions: Vec<ClaudeSession> = serde_json::from_value(serde_json::json!([
        {"id":"job1", "pid":123, "sessionId":id},
        {"id":"job2", "pid":456},
        {"pid":789, "sessionId":"Session_v2-A"},
        {"pid":890, "sessionId":"../escape"}
    ]))
    .unwrap();
    assert_eq!(claude_session_id(&sessions, 123).as_deref(), Some(id));
    assert_eq!(claude_session_id(&sessions, 456), None);
    assert_eq!(
        claude_session_id(&sessions, 789).as_deref(),
        Some("Session_v2-A")
    );
    assert_eq!(claude_session_id(&sessions, 890), None);
    assert_eq!(claude_session_id(&sessions, 999), None);
    let mut conflicting = sessions;
    conflicting.push(ClaudeSession {
        pid: Some(123),
        session_id: Some("Another_session-v2".into()),
    });
    assert_eq!(claude_session_id(&conflicting, 123), None);
}
