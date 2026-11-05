use super::{
    RoleRuntime,
    model::{HomeLayout, Identity, RoleConfig},
};
use crate::test_support::TestServices;
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

fn config(source: &Path) -> RoleConfig {
    RoleConfig {
        name: "Identity import".into(),
        description: String::new(),
        runtime: RoleRuntime::Cli {
            agent_id: "role-test-host".into(),
        },
        home_layout: HomeLayout::Custom,
        identity: Identity::LocalHome {
            source: Some(source.to_string_lossy().into_owned()),
        },
        instruction: String::new(),
        skills: vec![],
    }
}

#[tokio::test]
async fn builtin_identity_preserves_opaque_bytes_without_importing_configuration_or_history() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("home");
    std::fs::create_dir_all(source.join(".codex/sessions")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), [0, 255, 17]).unwrap();
    std::fs::write(source.join(".codex/config.toml"), "local settings").unwrap();
    std::fs::write(source.join(".codex/sessions/old"), "history").unwrap();
    symlink("/missing/socket", source.join("unselected-link")).unwrap();
    let manager = TestServices::persistent(&directory.path().join("aow")).unwrap();
    super::tests::configure_default_cli(&manager).await;
    let role = manager
        .roles
        .save_role(None, config(&source))
        .await
        .unwrap();
    let home = manager.roles.role_home(&role.id).unwrap();
    let identity = home.join(".codex/auth.json");
    assert_eq!(std::fs::read(&identity).unwrap(), [0, 255, 17]);
    for path in [".codex/config.toml", ".codex/sessions", "unselected-link"] {
        assert!(home.join(path).symlink_metadata().is_err(), "{path}");
    }
    assert_eq!(
        identity.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::write(&identity, "runtime refresh").unwrap();
    std::fs::remove_dir_all(&source).unwrap();
    manager
        .roles
        .save_role(Some(role.id), role.config)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(identity).unwrap(),
        "runtime refresh"
    );
}

#[tokio::test]
async fn credential_symlinks_are_copied_as_independent_files() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("home");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    let linked = directory.path().join("identity");
    std::fs::write(&linked, "original").unwrap();
    symlink(&linked, source.join(".codex/auth.json")).unwrap();
    let manager = TestServices::persistent(&directory.path().join("aow")).unwrap();
    super::tests::configure_default_cli(&manager).await;
    let role = manager
        .roles
        .save_role(None, config(&source))
        .await
        .unwrap();
    let imported = manager
        .roles
        .role_home(&role.id)
        .unwrap()
        .join(".codex/auth.json");
    assert!(
        !imported
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::write(imported, "refreshed").unwrap();
    assert_eq!(std::fs::read_to_string(linked).unwrap(), "original");
}

#[tokio::test]
async fn invalid_source_or_missing_builtin_file_does_not_publish_a_role_or_partial_home() {
    let directory = tempfile::tempdir().unwrap();
    let manager = TestServices::persistent(&directory.path().join("aow")).unwrap();
    super::tests::configure_default_cli(&manager).await;
    let source = directory.path().join("home");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    for path in [
        Path::new("relative"),
        Path::new(""),
        &directory.path().join("missing"),
        &source,
    ] {
        assert!(manager.roles.save_role(None, config(path)).await.is_err());
    }
    std::fs::create_dir(source.join(".codex/auth.json")).unwrap();
    assert!(
        manager
            .roles
            .save_role(None, config(&source))
            .await
            .is_err()
    );
    assert!(manager.roles.agent_roles().unwrap().is_empty());
    for entry in std::fs::read_dir(
        manager
            .node
            .agent_node()
            .await
            .unwrap()
            .node_environment()
            .await
            .unwrap()
            .state_dir
            .join("data/environments/roles"),
    )
    .unwrap()
    {
        assert_eq!(std::fs::read_dir(entry.unwrap().path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn all_advertised_agents_have_builtin_identity_files_and_accept_only_source_home() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("home");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    std::fs::create_dir_all(source.join(".claude")).unwrap();
    std::fs::create_dir_all(source.join(".trae/cli/sessions")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "codex").unwrap();
    std::fs::write(source.join(".claude/.credentials.json"), "claude").unwrap();
    std::fs::write(source.join(".claude/settings.json"), "local config").unwrap();
    std::fs::write(source.join(".trae/cli/auth.json"), "traecli").unwrap();
    std::fs::write(source.join(".trae/cli/config.toml"), "local config").unwrap();
    std::fs::write(source.join(".trae/cli/sessions/old"), "history").unwrap();
    let manager = TestServices::persistent(&directory.path().join("aow")).unwrap();
    super::tests::configure_default_cli(&manager).await;
    let settings = manager.node.acp_settings().unwrap();
    let agents = ["codex-acp", "claude-acp", "traecli-acp"]
        .into_iter()
        .map(|id| {
            (
                id.to_owned(),
                serde_json::json!({"type":"custom", "command":"/bin/echo"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    manager
        .node
        .save_acp_settings(
            serde_json::json!({"agent_servers":agents}).to_string(),
            settings.revision,
        )
        .await
        .unwrap();
    let presets = manager.roles.node_role_identity_sources().await.unwrap();
    assert_eq!(presets.len(), 3);
    for preset in presets {
        let mut input = config(&source);
        let (agent_id, expected, excluded) = match preset.layout {
            HomeLayout::Codex => ("codex-acp", ".codex/auth.json", ".claude/.credentials.json"),
            HomeLayout::Claude => (
                "claude-acp",
                ".claude/.credentials.json",
                ".codex/auth.json",
            ),
            HomeLayout::Traecli => ("traecli-acp", ".trae/cli/auth.json", ".codex/auth.json"),
            _ => panic!("Unexpected supported identity"),
        };
        input.runtime = RoleRuntime::Acp {
            agent_id: agent_id.into(),
        };
        let mut body = serde_json::to_value(&input).unwrap();
        body["identity"]["files"] = serde_json::json!([".claude/settings.json"]);
        assert!(
            serde_json::from_value::<RoleConfig>(body).is_err(),
            "clients cannot override identity files"
        );
        let role = manager.roles.save_role(None, input).await.unwrap();
        assert_eq!(role.config.home_layout, preset.layout);
        let home = manager.roles.role_home(&role.id).unwrap();
        assert!(home.join(expected).is_file());
        assert!(!home.join(excluded).exists());
        assert!(!home.join(".claude/settings.json").exists());
        assert!(!home.join(".trae/cli/config.toml").exists());
        assert!(!home.join(".trae/cli/sessions").exists());
        if preset.layout == HomeLayout::Traecli {
            let env = manager.roles.role_environment(&role).unwrap();
            assert_eq!(
                Path::new(&env["TRAE_HOME"]).join("cli/auth.json"),
                home.join(expected)
            );
            assert_eq!(
                Path::new(&env["TRAECLI_HOME"]).join("auth.json"),
                home.join(expected)
            );
        }
    }
}

#[tokio::test]
async fn skills_copy_their_assets_separately_from_identity() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("home");
    let skill = directory.path().join("review");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    std::fs::create_dir_all(skill.join("packages")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "identity").unwrap();
    std::fs::write(skill.join("SKILL.md"), "Review").unwrap();
    std::fs::write(skill.join("packages/asset"), "skill content").unwrap();
    let manager = TestServices::persistent(&directory.path().join("aow")).unwrap();
    super::tests::configure_default_cli(&manager).await;
    let mut input = config(&source);
    input.skills = vec![skill.to_string_lossy().into_owned()];
    let role = manager.roles.save_role(None, input).await.unwrap();
    assert!(
        manager
            .roles
            .role_home(&role.id)
            .unwrap()
            .join(".codex/skills/review/packages/asset")
            .is_file()
    );
}
