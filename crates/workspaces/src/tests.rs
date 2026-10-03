use std::{path::Path, process::Command};

use anyhow::{Result, ensure};

use crate::{GitExecutor, WorkspaceConfig, WorkspaceMode, prepare};

struct Git;
impl GitExecutor for Git {
    async fn output(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git").current_dir(cwd).args(args).output()?;
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }
}

#[test]
fn configuration_has_exactly_three_modes_and_validates_required_fields() {
    for mode in ["new_worktree", "existing", "temporary"] {
        assert!(
            serde_json::from_value::<WorkspaceConfig>(serde_json::json!({"workspace_mode":mode}))
                .is_ok()
        );
    }
    assert!(
        serde_json::from_value::<WorkspaceConfig>(
            serde_json::json!({"workspace_mode":"new_branch"})
        )
        .is_err()
    );
    for branch in ["", "-main", "main\n"] {
        assert!(
            WorkspaceConfig {
                base_branch: branch.into(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        WorkspaceConfig {
            workspace_mode: WorkspaceMode::Existing,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        WorkspaceConfig {
            workspace_mode: WorkspaceMode::Temporary,
            base_branch: String::new(),
            ..Default::default()
        }
        .validate()
        .is_ok()
    );
}

#[tokio::test]
async fn temporary_directory_survives_prepared_workspace_drop_without_git() {
    let config = WorkspaceConfig {
        workspace_mode: WorkspaceMode::Temporary,
        ..Default::default()
    };
    let prepared = prepare(
        &config,
        Path::new("/missing/repository"),
        "inbox/task",
        &Git,
    )
    .await
    .unwrap();
    assert!(prepared.created);
    assert!(prepared.branch.is_none());
    assert_eq!(std::fs::read_dir(&prepared.directory).unwrap().count(), 0);
    let path = prepared.directory.clone();
    drop(prepared);
    assert!(path.is_dir());
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn worktrees_use_selected_reference_and_sibling_directory_without_changing_main() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("main repo");
    std::fs::create_dir(&repository).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
        vec!["branch", "feature/base"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "next",
        ],
    ] {
        Git.output(&repository, &args).await.unwrap();
    }
    let config = WorkspaceConfig {
        base_branch: "feature/base".into(),
        ..Default::default()
    };
    let prepared = prepare(&config, &repository, "automation/task/run", &Git)
        .await
        .unwrap();
    assert!(prepared.created);
    assert_eq!(
        prepared.directory.parent(),
        repository.canonicalize().unwrap().parent()
    );
    assert_eq!(
        Git.output(&prepared.directory, &["rev-parse", "HEAD"])
            .await
            .unwrap(),
        Git.output(&repository, &["rev-parse", "feature/base"])
            .await
            .unwrap()
    );
    assert_eq!(
        Git.output(&repository, &["branch", "--show-current"])
            .await
            .unwrap()
            .trim(),
        "main"
    );
    let retried = prepare(&config, &repository, "automation/task/run", &Git)
        .await
        .unwrap();
    assert!(!retried.created);
    assert_eq!(retried.directory, prepared.directory);
    let selected = prepare(
        &WorkspaceConfig {
            workspace_mode: WorkspaceMode::Existing,
            workspace_path: prepared.directory.clone(),
            ..Default::default()
        },
        &repository,
        "unused",
        &Git,
    )
    .await
    .unwrap();
    assert!(!selected.created);
    assert_eq!(selected.branch.as_deref(), Some("automation/task/run"));
    let nested = prepared.directory.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let nested_workspace = prepare(
        &WorkspaceConfig {
            workspace_mode: WorkspaceMode::Existing,
            workspace_path: nested.clone(),
            ..Default::default()
        },
        &repository,
        "unused",
        &Git,
    )
    .await
    .unwrap();
    assert_eq!(nested_workspace.directory, nested);
    Git.output(&nested, &["init", "-b", "foreign"])
        .await
        .unwrap();
    assert!(
        prepare(
            &WorkspaceConfig {
                workspace_mode: WorkspaceMode::Existing,
                workspace_path: nested,
                ..Default::default()
            },
            &repository,
            "unused",
            &Git
        )
        .await
        .is_err()
    );
    let directory = prepared.directory.clone();
    drop(prepared);
    assert!(
        directory.exists(),
        "shared preparation never cleans up a Worktree"
    );
}
