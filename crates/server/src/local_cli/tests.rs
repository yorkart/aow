use super::*;
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn inbox_cli_and_web_share_revisions_comments_events_and_soft_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = AppState::with_state_dir(
        PathBuf::new(),
        directory.path().to_path_buf(),
        directory.path().join("no-terminald.sock"),
    )
    .unwrap();
    state.auth = crate::auth::AuthService::disabled();
    let server = start_local_cli(state.clone(), directory.path())
        .await
        .unwrap();
    let client = TerminaldClient::new(directory.path().join("cli/cli.sock"));
    let mut changes = state.workspace_events.subscribe();
    let app = crate::build_router(state);
    let captured: Value = client
        .post_json(
            "/v1/inbox/items",
            &json!({"request_key":"capture-one","markdown":"# 需求\n\n原始正文"}),
        )
        .await
        .unwrap();
    let id = captured["id"].as_str().unwrap();
    let path = format!("/v1/inbox/items/{id}");
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    assert_eq!(client.get_json::<Value>(&path).await.unwrap(), captured);
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/inbox/items/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), 1024 * 1024).await.unwrap()
        )
        .unwrap(),
        captured
    );
    let updated: Value = client.put_json(&path, &json!({"expected_revision":1,"markdown":"# 修正需求","project_id":null,"label_ids":["todo"]})).await.unwrap();
    assert_eq!(updated["revision"], 2);
    assert!(matches!(
        client
            .put_json::<_, Value>(
                &path,
                &json!({"expected_revision":1,"markdown":"旧修改","project_id":null,"label_ids":[]})
            )
            .await,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::CONFLICT,
            ..
        })
    ));
    let comment: Value = client.post_json(&format!("{path}/comments"), &json!({"request_key":"result-one","author":{"type":"ai","name":"Codex"},"content":"## 实现结论\n\n验证通过"})).await.unwrap();
    assert_eq!(
        client
            .get_json::<Value>(&format!("{path}/comments"))
            .await
            .unwrap(),
        json!([comment.clone()])
    );
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/inbox/items/{id}/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), 1024 * 1024).await.unwrap()
        )
        .unwrap(),
        json!([comment])
    );
    assert_eq!(
        client.get_json::<Value>("/v1/inbox").await.unwrap()["comment_counts"][id],
        1
    );
    client
        .delete_json(&path, &json!({"expected_revision":2}))
        .await
        .unwrap();
    assert!(
        client.get_json::<Value>("/v1/inbox").await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        client.get_json::<Value>(&path).await,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::NOT_FOUND,
            ..
        })
    ));
    let config = aow_config::ConfigRepository::open(directory.path())
        .unwrap()
        .unwrap();
    let deleted = config
        .directory()
        .join("inbox/requirements/deleted")
        .join(id);
    assert!(deleted.join("requirement.json").is_file());
    assert!(
        std::fs::read_to_string(deleted.join("comments.jsonl"))
            .unwrap()
            .contains("验证通过")
    );
    server.shutdown().await;
}

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
