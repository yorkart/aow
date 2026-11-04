use super::{
    model::{HomeLayout, Identity, RoleConfig},
    *,
};
use crate::aow::AowManager;
use std::{
    collections::BTreeMap,
    os::unix::fs::{PermissionsExt, symlink},
};

fn config(source: Option<String>) -> RoleConfig {
    RoleConfig {
        name: "Reviewer".into(),
        description: String::new(),
        runtime: RoleRuntime::Cli {
            agent_id: "role-test-runtime".into(),
        },
        home_layout: HomeLayout::Codex,
        identity: Identity::LocalHome { source },
        instruction: "Review changes carefully".into(),
        skills: vec![],
    }
}

#[tokio::test]
async fn role_whitelist_rejects_custom_runtimes_and_mismatched_home_layouts() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(&directory.path().join("aow")).unwrap();
    configure_default_cli(&manager).await;
    let mut input = config(None);
    let mut custom = serde_json::to_value(&input).unwrap();
    custom["runtime"]["agent_type"] = "custom".into();
    assert!(serde_json::from_value::<RoleConfig>(custom).is_err());
    for id in ["unknown", "vendor.opencode", "pi"] {
        input.runtime = RoleRuntime::Acp {
            agent_id: id.into(),
        };
        assert!(manager.save_role(None, input.clone()).await.is_err());
    }
    input = config(None);
    input.home_layout = HomeLayout::Claude;
    assert!(manager.save_role(None, input).await.is_err());
    assert!(manager.agent_roles().unwrap().is_empty());
    assert!(!directory.path().join("aow/data/agent-roles").exists());

    let role = manager.save_role(None, config(None)).await.unwrap();
    let mut changed = role.config.clone();
    changed.runtime = RoleRuntime::Acp {
        agent_id: "claude-acp".into(),
    };
    changed.home_layout = HomeLayout::Custom;
    assert!(
        manager
            .save_role(Some(role.id.clone()), changed)
            .await
            .is_err()
    );
    assert_eq!(manager.agent_role(&role.id).unwrap().config, role.config);
}

#[tokio::test]
async fn homes_are_seeded_once_shared_by_role_and_persist_without_changing_runtime_settings() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(source.join(".codex")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "original-identity").unwrap();
    std::fs::create_dir(source.join("sessions")).unwrap();
    std::fs::write(source.join("sessions/old"), "history").unwrap();
    let plugin_skills = directory.path().join("plugin-skills");
    std::fs::create_dir(&plugin_skills).unwrap();
    std::fs::write(plugin_skills.join("SKILL.md"), "not selected").unwrap();
    symlink(plugin_skills, source.join("skills")).unwrap();
    let manager = AowManager::persistent(&directory.path().join("aow")).unwrap();
    configure_default_cli(&manager).await;
    let initial_acp = manager.acp_settings().unwrap().content;
    let initial_agents = manager.agents().await.unwrap().len();
    let role = manager
        .save_role(None, config(Some(source.to_string_lossy().into_owned())))
        .await
        .unwrap();
    let home = manager.role_home(&role.id).unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        "original-identity"
    );
    assert!(!home.join(".codex/sessions").exists());
    assert!(!home.join(".codex/skills/SKILL.md").exists());
    assert_eq!(home.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    std::fs::write(home.join(".codex/auth.json"), "refreshed-identity").unwrap();
    std::fs::remove_dir_all(&source).unwrap();
    let first = manager.role_cli_launch(&role).await.unwrap();
    let second = manager.role_cli_launch(&role).await.unwrap();
    assert_eq!(first.env["HOME"], second.env["HOME"]);
    assert_eq!(
        first.env["CODEX_HOME"],
        home.join(".codex").to_string_lossy()
    );
    assert!(
        first
            .args
            .iter()
            .any(|arg| arg.contains("developer_instructions="))
    );
    let mut updated = role.config.clone();
    updated.name = "Reviewer renamed".into();
    manager
        .save_role(Some(role.id.clone()), updated)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        "refreshed-identity"
    );
    let other = manager.save_role(None, config(None)).await.unwrap();
    assert_ne!(
        manager.role_environment(&other).unwrap()["HOME"],
        first.env["HOME"]
    );
    let reopened = AowManager::persistent(&directory.path().join("aow")).unwrap();
    assert_eq!(reopened.agent_roles().unwrap().len(), 2);
    assert_eq!(reopened.role_home(&role.id).unwrap(), home);
    assert_eq!(manager.acp_settings().unwrap().content, initial_acp);
    assert_eq!(manager.agents().await.unwrap().len(), initial_agents);
    manager.remove_role(&role.id).await.unwrap();
    assert!(home.join(".codex/auth.json").is_file());
}

#[tokio::test]
async fn skill_edits_reconcile_only_injected_skills_and_roll_back_failed_updates() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("review");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), "original").unwrap();
    let manager = AowManager::persistent(&directory.path().join("aow")).unwrap();
    configure_default_cli(&manager).await;
    let mut input = config(None);
    input.skills.push(source.to_string_lossy().into_owned());
    let role = manager.save_role(None, input).await.unwrap();
    let skills = manager.role_home(&role.id).unwrap().join(".codex/skills");
    std::fs::write(skills.join("review/SKILL.md"), "role edit").unwrap();
    std::fs::create_dir(skills.join("personal")).unwrap();
    manager
        .save_role(Some(role.id.clone()), role.config.clone())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(skills.join("review/SKILL.md")).unwrap(),
        "role edit"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("SKILL.md")).unwrap(),
        "original"
    );
    let broken = directory.path().join("broken");
    std::fs::create_dir(&broken).unwrap();
    let mut changed = role.config.clone();
    changed.skills.push(broken.to_string_lossy().into_owned());
    assert!(
        manager
            .save_role(Some(role.id.clone()), changed)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(skills.join("review/SKILL.md")).unwrap(),
        "role edit"
    );
    assert_eq!(manager.agent_role(&role.id).unwrap().config, role.config);
    std::fs::write(broken.join("SKILL.md"), "new skill").unwrap();
    let mut expanded = role.config;
    expanded.skills.push(broken.to_string_lossy().into_owned());
    manager
        .save_role(Some(role.id.clone()), expanded.clone())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(skills.join("review/SKILL.md")).unwrap(),
        "role edit"
    );
    assert!(skills.join("broken/SKILL.md").is_file());
    let mut cleared = expanded;
    cleared.skills.clear();
    manager.save_role(Some(role.id), cleared).await.unwrap();
    assert!(!skills.join("review").exists());
    assert!(skills.join("personal").exists());
}

pub(super) async fn configure_cli(
    manager: &AowManager,
    agent_type: aow_agents::launch::AgentType,
    command: &str,
    args: Vec<String>,
    env: BTreeMap<String, String>,
) {
    let id = "role-test-runtime";
    let exists = manager
        .agents()
        .await
        .unwrap()
        .iter()
        .any(|agent| agent.id == id);
    manager
        .save_agent(
            exists.then_some(id),
            crate::aow::agents::RegisterAgentRequest {
                id: id.into(),
                agent_type,
                display_name: "Test runtime".into(),
                command: command.into(),
                args,
                env,
            },
        )
        .await
        .unwrap();
}

pub(super) async fn configure_default_cli(manager: &AowManager) {
    configure_cli(
        manager,
        aow_agents::launch::AgentType::Codex,
        "/bin/echo",
        vec![],
        BTreeMap::from([("CODEX_HOME".into(), "/wrong".into())]),
    )
    .await;
}

#[tokio::test]
async fn cli_roles_resolve_current_configuration_without_duplicating_or_mutating_it() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(&directory.path().join("aow")).unwrap();
    configure_default_cli(&manager).await;
    let role = manager.save_role(None, config(None)).await.unwrap();
    assert_eq!(
        serde_json::to_value(&role).unwrap()["runtime"],
        serde_json::json!({"mode":"cli", "agent_id":"role-test-runtime"})
    );
    let first = manager.role_cli_launch(&role).await.unwrap();
    let env = BTreeMap::from([
        ("HOME".into(), "/original-home".into()),
        ("CODEX_HOME".into(), "/original-codex".into()),
        ("RUNTIME_OPTION".into(), "updated".into()),
    ]);
    configure_cli(
        &manager,
        aow_agents::launch::AgentType::Codex,
        "/bin/cat",
        vec!["--updated".into()],
        env.clone(),
    )
    .await;
    let next = manager.role_cli_launch(&role).await.unwrap();
    assert_ne!(next.executable, first.executable);
    assert_eq!(next.args[0], "--updated");
    assert_eq!(next.env["RUNTIME_OPTION"], "updated");
    assert_eq!(next.env["HOME"], first.env["HOME"]);
    assert_eq!(next.env["CODEX_HOME"], first.env["CODEX_HOME"]);
    let base = manager
        .agents()
        .await
        .unwrap()
        .into_iter()
        .find(|agent| agent.id == "role-test-runtime")
        .unwrap();
    assert_eq!(base.env, env);
    assert_eq!(base.args, ["--updated"]);
    assert_eq!(manager.agent_role(&role.id).unwrap().config, role.config);
    let reopened = AowManager::persistent(&directory.path().join("aow")).unwrap();
    assert_eq!(reopened.role_cli_launch(&role).await.unwrap().env, next.env);
}

#[tokio::test]
async fn cli_roles_reject_missing_unconfigured_unavailable_and_changed_runtime_types() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(&directory.path().join("aow")).unwrap();
    assert!(manager.save_role(None, config(None)).await.is_err());
    // A detected executable is not a saved runtime definition.
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    symlink("/bin/echo", bin.join("codex")).unwrap();
    manager
        .update_settings(crate::aow::settings::UpdateSettingsRequest {
            execution_path: Some(vec![bin]),
            ..Default::default()
        })
        .await
        .unwrap();
    let detected = manager.agents().await.unwrap();
    assert!(
        detected
            .iter()
            .any(|agent| agent.id == "codex" && agent.source == "detected")
    );
    assert!(manager.role_runtime_cli_launch("codex").await.is_err());
    configure_default_cli(&manager).await;
    let role = manager.save_role(None, config(None)).await.unwrap();
    configure_cli(
        &manager,
        aow_agents::launch::AgentType::Codex,
        "/missing/role-runtime",
        vec![],
        BTreeMap::new(),
    )
    .await;
    assert!(manager.role_cli_launch(&role).await.is_err());
    assert!(manager.save_role(None, config(None)).await.is_err());
    configure_cli(
        &manager,
        aow_agents::launch::AgentType::Claude,
        "/bin/echo",
        vec![],
        BTreeMap::new(),
    )
    .await;
    assert!(
        manager
            .role_cli_launch(&role)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("agent type changed")
    );
    configure_cli(
        &manager,
        aow_agents::launch::AgentType::Pi,
        "/bin/echo",
        vec![],
        BTreeMap::new(),
    )
    .await;
    let mut unsupported = config(None);
    unsupported.home_layout = HomeLayout::Custom;
    assert!(manager.save_role(None, unsupported).await.is_err());
    manager.remove_agent("role-test-runtime").await.unwrap();
    assert!(manager.role_cli_launch(&role).await.is_err());
    assert!(manager.role_home(&role.id).unwrap().is_dir());
}

#[tokio::test]
async fn node_environment_updates_reconcile_unfinished_edits_before_retrying() {
    let directory = tempfile::tempdir().unwrap();
    let node_root = directory.path().join("node");
    std::fs::create_dir(&node_root).unwrap();
    let runtime = aow_runtime_client::testing::TestRuntime::with_state_dir(
        &directory.path().join("runtime.json"),
        node_root.clone(),
    )
    .await;
    let client = aow_runtime_client::RuntimeClient::new(runtime.endpoint.clone());
    let mut manager = AowManager::persistent(&directory.path().join("server")).unwrap();
    manager.set_runtime_client(Some(client.clone()));
    let role = AgentRole {
        id: "remote-reviewer".into(),
        config: config(None),
    };
    client
        .role_environment(&super::node::spec(&role))
        .await
        .unwrap();
    let skill = directory.path().join("review");
    std::fs::create_dir(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "review").unwrap();
    let mut abandoned = role.clone();
    abandoned
        .config
        .skills
        .push(skill.to_string_lossy().into_owned());
    client
        .prepare_role_environment(
            &super::node::spec(&abandoned),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    // Server still has the old publication. A new edit first rolls back the orphan ticket.
    manager
        .prepare_node_role(&role, Some(&role))
        .await
        .unwrap()
        .commit()
        .await;
    assert!(
        !node_root
            .join("data/environments/roles/remote-reviewer/home/.codex/skills/review")
            .exists()
    );
    assert!(!directory.path().join("server/data/environments").exists());
    runtime.shutdown().await.unwrap();
}
