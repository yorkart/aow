use super::*;

#[test]
fn saved_agent_ids_match_registered_agents_and_supported_capabilities() {
    for (id, agent) in [
        ("codex", Agent::Codex),
        ("traecli", Agent::TraeCli),
        ("claude", Agent::Claude),
    ] {
        let saved = format!("\"{id}\"");
        let adapter: AutomationAgent = serde_json::from_str(&saved).unwrap();
        assert_eq!(adapter.id(), id);
        assert_eq!(agent.automation(), Some(adapter));
        assert_eq!(serde_json::to_string(&adapter).unwrap(), saved);
    }
    for id in ["trae", "traex", "trae_cli", "gemini", "custom-agent"] {
        assert!(serde_json::from_value::<AutomationAgent>(serde_json::json!(id)).is_err());
    }
    assert!(Agent::Gemini.automation().is_none());
}

#[test]
fn each_adapter_enforces_new_sessions_and_uses_its_own_session_protocol() {
    for agent in [
        AutomationAgent::Codex,
        AutomationAgent::TraeCli,
        AutomationAgent::Claude,
    ] {
        assert!(
            agent
                .validate_arguments(&["--model".into(), "example-model".into()])
                .is_ok()
        );
        for forbidden in [
            "resume",
            "--resume=old-session",
            "--session-id=old-session",
            "--ephemeral",
            "--json",
        ] {
            assert!(
                agent.validate_arguments(&[forbidden.into()]).is_err(),
                "{agent:?} accepted {forbidden}"
            );
        }
    }
    let native_event = br#"{"type":"thread.started","thread_id":"native-session"}"#;
    for agent in [AutomationAgent::Codex, AutomationAgent::TraeCli] {
        assert_eq!(agent.session_id_mode(), SessionIdMode::FromOutput);
        assert_eq!(
            agent.session_from_line(native_event).as_deref(),
            Some("native-session")
        );
        assert_eq!(
            agent
                .session_from_line(b"session id: native-session\n")
                .as_deref(),
            Some("native-session")
        );
        assert!(
            agent
                .session_from_line(br#"{"type":"item.started","thread_id":"wrong"}"#)
                .is_none()
        );
    }
    let claude = AutomationAgent::Claude;
    assert_eq!(claude.session_id_mode(), SessionIdMode::GeneratedUuid);
    assert!(claude.session_from_line(native_event).is_none());
    assert!(claude.automation_arguments(true, None).is_err());
    for flag in ["-c", "-rold-session", "-ptext"] {
        assert!(claude.validate_arguments(&[flag.into()]).is_err());
    }
}
