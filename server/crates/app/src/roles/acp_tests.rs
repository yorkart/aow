use super::{
    RoleRuntime,
    model::{HomeLayout, Identity, RoleConfig},
};
use crate::AppState;
use crate::test_support::TestServices;
use aow_host_client::acp::{Content, SessionSource};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn fixture() -> (tempfile::TempDir, AppState, super::AgentRole) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("aow");
    let mut state = AppState::for_test(Default::default());
    state.replace_services(TestServices::persistent(&root).unwrap());
    let settings = state.node.acp_settings().unwrap();
    state.node.save_acp_settings(json!({"agent_servers":{"codex.test":{"type":"custom", "command":"node", "args":[
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/acp/lineage-agent.mjs")
    ]}}}).to_string(), settings.revision).await.unwrap();
    state.acp = crate::acp::service(
        &state.projects,
        &state.notifications,
        &state.cli_access,
        &state.node,
        Some(root),
        state.terminals.task_stop_sender(),
        state.workspace_events.clone(),
    )
    .unwrap();
    let role = state
        .roles
        .save_role(
            None,
            RoleConfig {
                name: "Reviewer".into(),
                description: String::new(),
                runtime: RoleRuntime::Acp {
                    agent_id: "codex.test".into(),
                },
                home_layout: HomeLayout::Custom,
                identity: Identity::LocalHome { source: None },
                instruction: "Review carefully".into(),
                skills: vec![],
            },
        )
        .await
        .unwrap();
    (directory, state, role)
}

fn content() -> Vec<Content> {
    serde_json::from_value(json!([{"type":"text", "text":"hello"}])).unwrap()
}

async fn idle(state: &AppState, id: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while state.acp.snapshot(id).await.unwrap().status != "idle" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn role_policy_is_owned_by_the_application_and_survives_edits_removal_and_restart() {
    let (directory, mut state, role) = fixture().await;
    let cwd = directory.path().to_str().unwrap();
    let prepared = state
        .roles
        .prepare_role_session(&role, &state, cwd, None, None)
        .await
        .unwrap();
    let id = prepared.resource().1.to_owned();
    crate::acp::start_task(state.clone(), id.clone(), Some("hello".into()))
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    idle(&state, &id).await;
    let snapshot = state.acp.snapshot(&id).await.unwrap();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert!(encoded.get("role_execution").is_none());
    assert!(encoded["execution_environment"].get("role_id").is_none());
    assert!(
        encoded["execution_environment"]
            .get("instruction")
            .is_none()
    );
    assert_eq!(
        snapshot.execution_environment.unwrap().home,
        state.roles.role_home(&role.id).unwrap()
    );
    let mut config = role.config.clone();
    config.instruction = "New instructions".into();
    let changed = state
        .roles
        .save_role(Some(role.id.clone()), config)
        .await
        .unwrap();
    let second = state
        .roles
        .prepare_role_session(&changed, &state, cwd, None, None)
        .await
        .unwrap();
    let second_id = second.resource().1;
    state.acp.resume(second_id).await.unwrap();
    assert_eq!(
        state
            .acp
            .session_connection_info(&id)
            .await
            .unwrap()
            .unwrap()
            .id,
        state
            .acp
            .session_connection_info(second_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        "instructions do not decide which adapter process is reused"
    );
    assert!(
        state
            .tasks
            .acp_role_content(&state.acp, second_id, content())
            .await
            .unwrap()[0]
            .data["text"]
            .as_str()
            .unwrap()
            .contains("New instructions")
    );
    let parent = aow_host_contract::instance::InstanceRef::acp(&id);
    let child = state
        .roles
        .prepare_acp_child_session(
            &state.acp,
            "codex.test",
            directory.path().into(),
            SessionSource::Cli,
            Some(parent),
        )
        .await
        .unwrap();
    assert_eq!(
        child.execution_environment,
        state.acp.snapshot(&id).await.unwrap().execution_environment
    );
    assert!(
        state
            .tasks
            .acp_role_content(&state.acp, &child.thread_id, content())
            .await
            .unwrap()[0]
            .data["text"]
            .as_str()
            .unwrap()
            .contains("Review carefully")
    );
    let fork = state.acp.fork_session(&id).await.unwrap();
    assert!(
        state
            .tasks
            .acp_role_content(&state.acp, &fork.thread_id, content())
            .await
            .unwrap()[0]
            .data["text"]
            .as_str()
            .unwrap()
            .contains("Review carefully")
    );
    // Calling the executor directly never adds the application's instructions.
    state.acp.start_prompt(&id, content()).await.unwrap();
    idle(&state, &id).await;
    let prompts: Vec<Value> =
        std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(
        prompts[0]["prompt"],
        json!([{"type":"text","text":"Agent role instructions:\nReview carefully\n\n","_meta":{"aow/instructions":true}},{"type":"text","text":"hello"}])
    );
    assert_eq!(
        prompts[1]["prompt"],
        json!([{"type":"text","text":"hello"}])
    );
    state.roles.remove_role(&role.id).await.unwrap();
    state.acp.flush_threads_on_quit().await.unwrap();
    let root = directory.path().join("aow");
    state.replace_services(TestServices::persistent(&root).unwrap());
    state.acp = crate::acp::service(
        &state.projects,
        &state.notifications,
        &state.cli_access,
        &state.node,
        Some(root),
        state.terminals.task_stop_sender(),
        state.workspace_events.clone(),
    )
    .unwrap();
    crate::acp::start_task(state.clone(), id.clone(), Some("after restart".into()))
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    idle(&state, &id).await;
    let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
    let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(last["home"], prompts[0]["home"]);
    assert_eq!(last["prompt"][0], prompts[0]["prompt"][0]);
}

#[tokio::test]
async fn role_binding_write_failure_discards_the_prepared_acp_session() {
    let (directory, state, role) = fixture().await;
    std::fs::create_dir(directory.path().join("aow").join(super::SESSIONS_FILE)).unwrap();
    assert!(
        state
            .roles
            .prepare_role_session(
                &role,
                &state,
                directory.path().to_str().unwrap(),
                None,
                None
            )
            .await
            .is_err()
    );
    assert!(state.acp.sessions(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn acp_roles_follow_shared_configuration_and_reject_removed_or_unavailable_runtimes() {
    let (directory, state, role) = fixture().await;
    let cwd = directory.path().to_str().unwrap();
    let home = state.roles.role_home(&role.id).unwrap();
    for setting in ["first", "updated"] {
        let previous = state.node.acp_settings().unwrap();
        let mut config: Value = serde_json::from_str(&previous.content).unwrap();
        config["agent_servers"]["codex.test"]["env"] = json!({
            "CUSTOM_AGENT_SETTING": setting, "HOME":"/wrong", "CODEX_HOME":"/wrong"
        });
        let content = config.to_string();
        state
            .node
            .save_acp_settings(content.clone(), previous.revision)
            .await
            .unwrap();
        let session = state
            .roles
            .prepare_role_session(&role, &state, cwd, None, None)
            .await
            .unwrap();
        let id = session.resource().1;
        crate::acp::start_task(state.clone(), id.into(), Some("hello".into()))
            .unwrap()
            .await
            .unwrap()
            .unwrap();
        idle(&state, id).await;
        let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl")).unwrap();
        let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
        assert_eq!(last["setting"], setting);
        assert_eq!(last["home"], home.to_string_lossy().as_ref());
        assert_eq!(state.node.acp_settings().unwrap().content, content);
    }
    for config in [
        json!({"agent_servers":{"codex.test":{"type":"registry"}}}),
        json!({"agent_servers":{}}),
    ] {
        let previous = state.node.acp_settings().unwrap();
        state
            .node
            .save_acp_settings(config.to_string(), previous.revision)
            .await
            .unwrap();
        assert!(
            state
                .roles
                .prepare_role_session(&role, &state, cwd, None, None)
                .await
                .is_err()
        );
        assert!(
            state
                .roles
                .save_role(None, role.config.clone())
                .await
                .is_err()
        );
        assert_eq!(state.acp.sessions(None).await.unwrap().len(), 2);
        assert!(home.is_dir());
    }
    for session in state.acp.sessions(None).await.unwrap() {
        if let Some(connection) = state
            .acp
            .session_connection_info(&session.thread_id)
            .await
            .unwrap()
        {
            let _ = state.acp.disconnect(&connection.id).await;
        }
    }
}
