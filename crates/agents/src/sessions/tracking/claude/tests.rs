use super::*;

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

#[cfg(unix)]
#[tokio::test]
async fn native_lookup_is_cached_per_environment_and_failure_is_not_an_empty_result() {
    let first = tempfile::tempdir().unwrap();
    let id = "550e8400-e29b-41d4-a716-446655440000";
    let environment = cli_fixture(
        first.path(),
        &format!(r#"[{{"pid":123,"id":"short-job","sessionId":"{id}"}}]"#),
    );
    let context = || LiveSessionContext {
        pid: Some(123),
        cwd: "/workspace/demo",
        title: "A plausible title",
        environment: &environment,
    };
    let tracker = Agent::Claude.session_tracking().unwrap();
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::Resolved(SessionTarget::Id(id.into()))
    );
    // Several panes/clients reuse the same CLI result, including PID misses.
    std::fs::write(first.path().join("config/active.json"), "[]").unwrap();
    assert_eq!(
        tracker.resolve_live_session(context()).await,
        SessionResolution::Resolved(SessionTarget::Id(id.into()))
    );
    assert_eq!(
        tracker
            .resolve_live_session(LiveSessionContext {
                pid: Some(456),
                ..context()
            })
            .await,
        SessionResolution::NotFound
    );
    assert_eq!(
        std::fs::read_to_string(first.path().join("config/queries")).unwrap(),
        "x"
    );

    let second = tempfile::tempdir().unwrap();
    let unavailable = cli_fixture(second.path(), "invalid JSON");
    assert_eq!(
        tracker
            .resolve_live_session(LiveSessionContext {
                environment: &unavailable,
                ..context()
            })
            .await,
        SessionResolution::Unavailable
    );
    // A failed query is cached too, and cannot turn into title matching.
    std::fs::write(second.path().join("config/active.json"), "[]").unwrap();
    assert_eq!(
        tracker
            .resolve_live_session(LiveSessionContext {
                environment: &unavailable,
                ..context()
            })
            .await,
        SessionResolution::Unavailable
    );
    assert_eq!(
        std::fs::read_to_string(second.path().join("config/queries")).unwrap(),
        "x"
    );

    let third = tempfile::tempdir().unwrap();
    let empty = cli_fixture(third.path(), "[]");
    assert_eq!(
        tracker
            .resolve_live_session(LiveSessionContext {
                environment: &empty,
                ..context()
            })
            .await,
        SessionResolution::NotFound
    );
}

#[test]
fn claude_uses_full_uuid_for_the_exact_pid_and_rejects_ambiguity() {
    let id = "550e8400-e29b-41d4-a716-446655440000";
    let sessions: Vec<ClaudeSession> = serde_json::from_value(serde_json::json!([
        {"id":"job1", "pid":123, "sessionId":id},
        {"id":"job2", "pid":456},
        {"pid":789, "sessionId":"short-job-id"}
    ]))
    .unwrap();
    assert_eq!(claude_session_id(&sessions, 123).as_deref(), Some(id));
    assert_eq!(claude_session_id(&sessions, 456), None);
    assert_eq!(claude_session_id(&sessions, 789), None);
    assert_eq!(claude_session_id(&sessions, 999), None);
    let mut conflicting = sessions;
    conflicting.push(ClaudeSession {
        pid: Some(123),
        session_id: Some(Uuid::new_v4().to_string()),
    });
    assert_eq!(claude_session_id(&conflicting, 123), None);
}
