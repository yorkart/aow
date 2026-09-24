use super::*;
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn project_queries_return_registry_metadata_without_git_or_terminald() {
    let directory = tempfile::tempdir().unwrap();
    let state =
        AppState::with_terminald_socket(PathBuf::new(), directory.path().join("no-terminald.sock"));
    let server = start_local_cli(state.clone(), directory.path())
        .await
        .unwrap();
    let client = TerminaldClient::new(directory.path().join("cli/cli.sock"));
    assert_eq!(
        client.get_json::<Value>("/v1/projects").await.unwrap(),
        json!({"items":[]})
    );

    let repo = directory.path().join("repo with spaces");
    std::fs::create_dir(&repo).unwrap();
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
    ] {
        let output = std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let app = crate::build_router(state);
    let response = app.clone().oneshot(
        Request::post("/api/aow/projects")
            .header("content-type", "application/json")
            .body(Body::from(json!({"path":repo, "name":"项目", "notes_path":directory.path().join("notes")}).to_string())).unwrap()
    ).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(
        status.is_success(),
        "{status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    let project: Value = serde_json::from_slice(&bytes).unwrap();
    let path = format!("/v1/projects/{}", project["id"].as_str().unwrap());
    let expected =
        json!({"id":project["id"], "name":"项目", "repo_path":repo.canonicalize().unwrap()});

    assert_eq!(
        client.get_json::<Value>("/v1/projects").await.unwrap(),
        json!({"items":[expected.clone()]})
    );
    assert_eq!(client.get_json::<Value>(&path).await.unwrap(), expected);

    // The response must remain registry-only even when Git cannot inspect the repository.
    std::fs::rename(&repo, directory.path().join("moved-repo")).unwrap();
    assert_eq!(
        client.get_json::<Value>("/v1/projects").await.unwrap(),
        json!({"items":[expected.clone()]})
    );
    assert_eq!(client.get_json::<Value>(&path).await.unwrap(), expected);

    assert!(matches!(
        client.get_json::<Value>("/v1/projects/missing").await,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::NOT_FOUND,
            ..
        })
    ));
    assert!(matches!(
        client
            .post_json::<_, Value>("/v1/projects", &json!({}))
            .await,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::METHOD_NOT_ALLOWED,
            ..
        })
    ));

    // Removing the registration is immediately reflected by both CLI queries.
    let response = app
        .oneshot(
            Request::delete(format!(
                "/api/aow/projects/{}",
                project["id"].as_str().unwrap()
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        client.get_json::<Value>("/v1/projects").await.unwrap(),
        json!({"items":[]})
    );
    assert!(matches!(
        client.get_json::<Value>(&path).await,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::NOT_FOUND,
            ..
        })
    ));
    server.shutdown().await;
}
