use super::super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn profile(id: &str) -> Value {
    json!({"id": id, "agent_type": "codex", "display_name": "Work",
        "command": "/bin/sh", "args": ["--model", "literal value"], "env": {"MODE": "work"}})
}

async fn call(app: &axum::Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
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
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes))),
    )
}

#[tokio::test]
async fn explicit_ids_are_required_unique_and_immutable() {
    let state = AppState::new(PathBuf::new());
    let app = crate::build_router(state.clone());
    for id in [
        "",
        "bad.id",
        "with space",
        " leading",
        "trailing ",
        "a/b",
        "中文",
        "line\n",
    ] {
        let (status, _) = call(&app, "POST", "/api/aow/agents", profile(id)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{id:?}");
    }
    let (status, _) = call(&app, "POST", "/api/aow/agents", profile(&"a".repeat(81))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut missing = profile("unused");
    missing.as_object_mut().unwrap().remove("id");
    assert_eq!(
        call(&app, "POST", "/api/aow/agents", missing).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    for id in ["Work_codex-2", "other"] {
        assert_eq!(
            call(&app, "POST", "/api/aow/agents", profile(id)).await.0,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        call(&app, "POST", "/api/aow/agents", profile("Work_codex-2"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    for id in ["other", "my-agent_3"] {
        let (status, error) = call(&app, "PUT", "/api/aow/agents/Work_codex-2", profile(id)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("创建后不可修改")
        );
    }
    assert_eq!(
        call(&app, "PUT", "/api/aow/agents/missing", profile("new"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut edited = profile("Work_codex-2");
    edited["display_name"] = json!("Renamed display");
    edited["args"] = json!(["--updated"]);
    edited["env"] = json!({"MODE": "updated"});
    let (status, saved) = call(&app, "PUT", "/api/aow/agents/Work_codex-2", edited.clone()).await;
    assert_eq!(status, StatusCode::OK);
    for key in ["id", "display_name", "args", "env"] {
        assert_eq!(saved[key], edited[key]);
    }
    let agents = state.aow.agents().await.unwrap();
    assert!(agents.iter().any(|agent| agent.id == "Work_codex-2"));
    assert!(!agents.iter().any(|agent| agent.id == "my-agent_3"));
    assert_eq!(
        call(&app, "DELETE", "/api/aow/agents/Work_codex-2", Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, saved) = call(&app, "POST", "/api/aow/agents", profile("my-agent_3")).await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    assert_eq!(saved["id"], "my-agent_3");
    assert_eq!(saved["args"], profile("unused")["args"]);
    let agents = state.aow.agents().await.unwrap();
    assert!(!agents.iter().any(|agent| agent.id == "Work_codex-2"));
    assert!(
        agents
            .iter()
            .any(|agent| agent.id == "other" && agent.display_name == "Work")
    );
}

#[tokio::test]
async fn simultaneous_creates_cannot_claim_the_same_id() {
    let app = crate::build_router(AppState::new(PathBuf::new()));
    let (left, right) = tokio::join!(
        call(&app, "POST", "/api/aow/agents", profile("unique-agent")),
        call(&app, "POST", "/api/aow/agents", profile("unique-agent"))
    );
    let mut statuses = [left.0.as_u16(), right.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
}

#[tokio::test]
async fn rejected_id_changes_and_delete_then_create_preserve_the_registry_on_disk() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    manager
        .register_agent(serde_json::from_value(profile("original")).unwrap())
        .await
        .unwrap();
    let result = manager
        .save_agent(
            Some("original"),
            serde_json::from_value(profile("renamed")).unwrap(),
        )
        .await;
    assert!(matches!(result, Err(AowError::Invalid(_))));
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    for manager in [&manager, &reloaded] {
        let agents = manager.agents().await.unwrap();
        assert!(agents.iter().any(|agent| agent.id == "original"));
        assert!(!agents.iter().any(|agent| agent.id == "renamed"));
    }
    manager.remove_agent("original").await.unwrap();
    manager
        .register_agent(serde_json::from_value(profile("replacement")).unwrap())
        .await
        .unwrap();
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    for manager in [&manager, &reloaded] {
        let agents = manager.agents().await.unwrap();
        assert!(!agents.iter().any(|agent| agent.id == "original"));
        assert!(agents.iter().any(|agent| agent.id == "replacement"));
    }
}
