use super::{
    AgentRole, RoleRuntime,
    model::{HomeLayout, Identity, RoleConfig},
};
use crate::AppState;
use crate::test_support::TestServices;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tower::ServiceExt;

async fn fixture() -> (tempfile::TempDir, AppState, RoleConfig) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    std::fs::create_dir_all(source.join(".claude")).unwrap();
    std::fs::create_dir_all(source.join(".trae/cli")).unwrap();
    std::fs::write(source.join(".trae/cli/auth.json"), "opaque identity bytes").unwrap();
    std::fs::write(source.join(".claude/.credentials.json"), "original").unwrap();
    std::fs::write(source.join(".codex/auth.json"), "original").unwrap();
    let mut state = AppState::for_test(Default::default());
    state.replace_services(TestServices::persistent(&directory.path().join("aow")).unwrap());
    let config = RoleConfig {
        name: "Reviewer".into(),
        description: "Reviews changes".into(),
        runtime: RoleRuntime::Cli {
            agent_id: "role-test-host".into(),
        },
        home_layout: HomeLayout::Custom,
        identity: Identity::LocalHome {
            source: Some(source.to_string_lossy().into_owned()),
        },
        instruction: String::new(),
        skills: vec![],
    };
    configure(
        &state,
        aow_host_contract::agents::AgentType::Codex,
        BTreeMap::new(),
    )
    .await;
    (directory, state, config)
}

fn script() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/role-connection.mjs")
        .to_string_lossy()
        .into_owned()
}

async fn create(app: &Router, config: &RoleConfig, id: &str) -> (StatusCode, serde_json::Value) {
    let mut body = serde_json::to_value(config).unwrap();
    body.as_object_mut().unwrap().remove("home_layout");
    body["request_id"] = id.into();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent-roles/test-and-create")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn tested_creation_uses_persistent_home_and_retries_without_reseeding_or_retesting() {
    let (directory, state, config) = fixture().await;
    let app = super::routes().with_state(state.clone());
    for (id, agent) in [
        ("codex", aow_host_contract::agents::AgentType::Codex),
        ("claude", aow_host_contract::agents::AgentType::Claude),
        ("traecli", aow_host_contract::agents::AgentType::TraeCli),
    ] {
        let mut env = BTreeMap::from([("PROBE_REPLY".into(), format!("Ready to help from {id}."))]);
        if agent == aow_host_contract::agents::AgentType::TraeCli {
            env.insert("PROBE_REQUIRED_HOME".into(), "TRAECLI_HOME".into());
            env.insert("PROBE_REQUIRED_FILE".into(), "auth.json".into());
        }
        configure(&state, agent, env).await;
        let (status, body) = create(&app, &config, id).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["description"], config.description);
        assert_eq!(body["home_layout"], id);
        let home = state.roles.role_home(id).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("tested")).unwrap(),
            "refreshed"
        );
        std::fs::write(home.join("tested"), "session change").unwrap();
        let (status, _) = create(&app, &config, id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            std::fs::read_to_string(home.join("tested")).unwrap(),
            "session change"
        );
        let mut changed = config.clone();
        changed.name = "different".into();
        assert_eq!(create(&app, &changed, id).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            std::fs::read_dir(home.parent().unwrap())
                .unwrap()
                .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_dir())
                .count(),
            1
        );
    }
    assert_eq!(state.roles.agent_roles().unwrap().len(), 3);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("source/.codex/auth.json")).unwrap(),
        "original"
    );
    assert!(!directory.path().join("source/tested").exists());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("source/.trae/cli/auth.json")).unwrap(),
        "opaque identity bytes"
    );
    assert!(state.acp.sessions(None).await.unwrap().is_empty());
    let reopened = TestServices::persistent(&directory.path().join("aow")).unwrap();
    assert_eq!(reopened.roles.agent_roles().unwrap().len(), 3);
}

#[tokio::test]
async fn missing_builtin_identity_retries_in_a_fresh_home_and_probe_reads_the_imported_file() {
    let (directory, state, config) = fixture().await;
    let app = super::routes().with_state(state.clone());
    let identity = directory.path().join("source/.codex/auth.json");
    std::fs::remove_file(&identity).unwrap();
    configure(
        &state,
        aow_host_contract::agents::AgentType::Codex,
        BTreeMap::from([("PROBE_REQUIRED_FILE".into(), ".codex/auth.json".into())]),
    )
    .await;
    let (status, body) = create(&app, &config, "identity-retry").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(state.roles.agent_roles().unwrap().is_empty());
    assert!(state.roles.role_home("identity-retry").is_err());
    std::fs::write(&identity, "opaque identity bytes").unwrap();
    let (status, body) = create(&app, &config, "identity-retry").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let home = state.roles.role_home("identity-retry").unwrap();
    assert!(home.join("tested").is_file());
    assert!(home.join(".codex/auth.json").is_file());
    assert!(!home.join(".claude/.credentials.json").exists());
}

#[tokio::test]
async fn failed_or_incomplete_cli_connections_do_not_publish_a_role_or_keep_candidate_home() {
    let (_directory, state, mut config) = fixture().await;
    let app = super::routes().with_state(state.clone());
    for mode in ["fail", "empty", "whitespace"] {
        configure(
            &state,
            aow_host_contract::agents::AgentType::Codex,
            BTreeMap::from([("PROBE_MODE".into(), mode.into())]),
        )
        .await;
        let (status, body) = create(&app, &config, mode).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(!body.to_string().contains("private provider error"));
        assert!(state.roles.role_home(mode).is_err());
        assert!(state.roles.agent_roles().unwrap().is_empty());
    }
    config.identity = Identity::LocalHome { source: None };
    assert_eq!(
        create(&app, &config, "missing").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn acp_probe_uses_shared_settings_without_leaving_workspace_sessions() {
    let (_directory, state, mut config) = fixture().await;
    let app = super::routes().with_state(state.clone());
    config.runtime = RoleRuntime::Acp {
        agent_id: "codex.probe".into(),
    };
    config.instruction = "Role instructions must not replace the ping probe".into();
    for mode in ["auth", "empty", "whitespace", "prompt-error", "success"] {
        let previous = state.node.acp_settings().unwrap();
        let settings = serde_json::json!({"agent_servers": {"codex.probe": {
            "type": "custom", "command": "node", "args": [script(), "--acp"], "env": {"PROBE_MODE": mode, "PROBE_REPLY": "Connected and ready to help."}
        }}}).to_string();
        state
            .node
            .save_acp_settings(settings.clone(), previous.revision)
            .await
            .unwrap();
        let (status, body) = create(&app, &config, "acp-probe").await;
        if mode != "success" {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert!(state.roles.role_home("acp-probe").is_err());
            assert!(state.roles.agent_roles().unwrap().is_empty());
        } else {
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["home_layout"], "codex");
            assert!(
                state
                    .roles
                    .role_home("acp-probe")
                    .unwrap()
                    .join("tested")
                    .is_file()
            );
        }
        assert_eq!(state.node.acp_settings().unwrap().content, settings);
        assert!(state.acp.sessions(None).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn timed_out_cli_is_terminated_and_scratch_workspace_is_removed() {
    let (directory, state, config) = fixture().await;
    let pid = directory.path().join("pid");
    let late = directory.path().join("late");
    configure(
        &state,
        aow_host_contract::agents::AgentType::Codex,
        BTreeMap::from([
            ("PROBE_MODE".into(), "timeout".into()),
            ("PROBE_PID".into(), pid.to_string_lossy().into_owned()),
            ("PROBE_LATE".into(), late.to_string_lossy().into_owned()),
        ]),
    )
    .await;
    let role: AgentRole = state.roles.save_role(None, config).await.unwrap();
    let error = super::connection::test_with_timeout(&state, &role, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(pid.is_file(), "fake runtime should have started");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!late.exists(), "runtime survived its timeout");
    let home = state.roles.role_home(&role.id).unwrap();
    assert_eq!(
        std::fs::read_dir(home.parent().unwrap())
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_dir())
            .count(),
        1
    );
}

async fn configure(
    state: &AppState,
    agent_type: aow_host_contract::agents::AgentType,
    env: BTreeMap<String, String>,
) {
    super::tests::configure_cli(state, agent_type, "node", vec![script()], env).await;
}
