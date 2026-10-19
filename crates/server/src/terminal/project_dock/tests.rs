use super::*;
use crate::terminal::tests::{start_daemon, stop_daemon};
use axum::{body::to_bytes, http::Request};
use serde_json::{Value, json};
use tower::ServiceExt;

fn shell_request(root: &str, name: &str) -> CreateTerminalRequest {
    serde_json::from_value(
        json!({"workspace_root": root, "cwd": root, "name": name, "shell": "/bin/sh"}),
    )
    .unwrap()
}

#[tokio::test]
async fn project_dock_is_atomic_persistent_and_recreated_only_after_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_str().unwrap();
    let socket = directory.path().join("terminald/socket");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let state_dir = directory.path().join("state");
    let manager = TerminalManager::persistent(state_dir.clone(), client.clone()).unwrap();
    let ordinary = manager
        .create(shell_request(root, "Project · Terminal"))
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        manager.project_dock("project", root, "Project · Terminal", None),
        manager.project_dock("project", root, "Project · Terminal", None),
    );
    let first = first.unwrap();
    assert_eq!(first.id, second.unwrap().id);
    assert_ne!(first.id, ordinary.id);
    assert_eq!(first.dock_project_id.as_deref(), Some("project"));
    assert!(
        manager
            .get_snapshot(&ordinary.id)
            .unwrap()
            .dock_project_id
            .is_none()
    );
    assert_eq!(client.list().await.unwrap().len(), 2);
    manager
        .update(
            &first.id,
            UpdateTerminalRequest {
                name: Some("Renamed".into()),
            },
        )
        .unwrap();
    drop(manager);
    let manager = TerminalManager::persistent(state_dir, client.clone()).unwrap();
    let restored = manager
        .project_dock("project", root, "New project name · Terminal", None)
        .await
        .unwrap();
    assert_eq!(restored.id, first.id);
    assert_eq!(restored.name, "Renamed");
    assert_eq!(restored.panes, first.panes);
    manager.delete_tab(&restored.id).await.unwrap();
    let replacement = manager
        .project_dock("project", root, "Project · Terminal", None)
        .await
        .unwrap();
    assert_ne!(replacement.id, first.id);
    assert_eq!(client.list().await.unwrap().len(), 2);
    stop_daemon(shutdown, daemon).await;
}

#[tokio::test]
async fn project_dock_adopts_only_an_explicit_valid_legacy_id_and_rolls_back_failed_creation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_str().unwrap();
    let socket = directory.path().join("terminald/socket");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let manager = TerminalManager::in_memory(client.clone());
    let old = manager.create(shell_request(root, "Legacy")).await.unwrap();
    assert!(
        manager
            .project_dock("wrong", "/", "Wrong", Some(&old.id))
            .await
            .is_err()
    );
    let adopted = manager
        .project_dock("project", root, "Project · Terminal", Some(&old.id))
        .await
        .unwrap();
    assert_eq!(adopted.id, old.id);
    assert_eq!(adopted.panes, old.panes);
    assert_eq!(adopted.name, old.name);
    assert!(
        manager
            .project_dock("another", root, "Another", Some(&old.id))
            .await
            .is_err()
    );
    let mut invalid = shell_request(root, "Failed");
    invalid.shell = Some("/missing/aow-test-shell".into());
    assert!(
        manager
            .create_shell(invalid, Some(("failed", None)))
            .await
            .is_err()
    );
    assert!(
        manager
            .lock_state()
            .unwrap()
            .tabs
            .iter()
            .all(|tab| tab.dock_project_id.as_deref() != Some("failed"))
    );
    let retry = manager
        .project_dock("failed", root, "Retry", None)
        .await
        .unwrap();
    assert_eq!(retry.dock_project_id.as_deref(), Some("failed"));
    assert_eq!(client.list().await.unwrap().len(), 2);
    stop_daemon(shutdown, daemon).await;
}

async fn post(app: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn project_dock_endpoint_uses_registered_project_defaults_and_rejects_unknown_projects() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    let socket = directory.path().join("terminald/socket");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let mut state = AppState::new(directory.path().join("frontend"));
    state.terminals = TerminalManager::in_memory(client.clone());
    let app = crate::build_router(state);
    let (status, project) = post(
        &app,
        "/api/aow/projects",
        json!({"path": root, "name": "Example", "notes_path": directory.path().join("notes")}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    let endpoint = format!(
        "/api/terminals/project-dock/{}",
        project["id"].as_str().unwrap()
    );
    let (first, second) = tokio::join!(
        post(&app, &endpoint, json!({})),
        post(&app, &endpoint, json!({}))
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(first.1["id"], second.1["id"]);
    assert_eq!(first.1["dock_project_id"], project["id"]);
    assert_eq!(first.1["name"], "Example · Terminal");
    assert_eq!(first.1["workspace_root"], root.to_str().unwrap());
    assert_eq!(first.1["panes"][0]["cwd"], root.to_str().unwrap());
    assert_eq!(
        post(&app, "/api/terminals/project-dock/unknown", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(client.list().await.unwrap().len(), 1);
    stop_daemon(shutdown, daemon).await;
}
