use super::super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
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

#[tokio::test]
async fn agent_order_uses_types_then_detected_then_configured_items_and_survives_reload() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    let registry = manager.inner.agents_path.as_ref().unwrap();
    let mut claude = profile("work-claude");
    claude["agent_type"] = json!("claude");
    let mut legacy_builtin = profile("pi");
    legacy_builtin.as_object_mut().unwrap().remove("agent_type");
    let mut legacy_unknown = profile("legacy");
    legacy_unknown.as_object_mut().unwrap().remove("agent_type");
    // Old documents may interleave types or omit agent_type. Loading never migrates them.
    let original = json!({"version": 1, "items": [profile("z-work"), legacy_unknown,
        claude, legacy_builtin, profile("a-work")]});
    std::fs::write(registry, serde_json::to_vec(&original).unwrap()).unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    for agent_type in AgentType::ALL {
        let executable = directory
            .path()
            .join(agent_type.agent().definition().commands[0]);
        std::fs::write(&executable, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let ids = |manager: &AowManager| {
        manager
            .agents_in_path(&[directory.path().to_path_buf()])
            .unwrap()
            .into_iter()
            .map(|agent| agent.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&manager),
        [
            "claude",
            "work-claude",
            "codex",
            "z-work",
            "a-work",
            "traecli",
            "hermes",
            "pi",
            "legacy"
        ]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(registry).unwrap()).unwrap(),
        original
    );
    manager
        .reorder_agents(ReorderAgentsRequest {
            agent_type: Some(AgentType::Codex),
            ids: vec!["a-work".into(), "z-work".into()],
        })
        .await
        .unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(registry).unwrap()).unwrap();
    assert_eq!(saved["version"], 1);
    assert_eq!(saved.as_object().unwrap().len(), 2);
    assert_eq!(
        saved["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["work-claude", "a-work", "z-work", "pi", "legacy"]
    );
    for before in original["items"].as_array().unwrap() {
        let after = saved["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == before["id"])
            .unwrap();
        for key in ["id", "display_name", "command", "args", "env", "agent_type"] {
            assert_eq!(after[key], before[key], "{key}");
        }
    }
    let reloaded = AowManager::persistent(directory.path()).unwrap();
    assert_eq!(ids(&manager), ids(&reloaded));
    assert_eq!(
        ids(&reloaded),
        [
            "claude",
            "work-claude",
            "codex",
            "a-work",
            "z-work",
            "traecli",
            "hermes",
            "pi",
            "legacy"
        ]
    );
    // Editing preserves position, and new profiles are appended within their type.
    manager
        .save_agent(
            Some("a-work"),
            serde_json::from_value(profile("a-work")).unwrap(),
        )
        .await
        .unwrap();
    manager
        .register_agent(serde_json::from_value(profile("new-work")).unwrap())
        .await
        .unwrap();
    assert_eq!(
        ids(&manager),
        [
            "claude",
            "work-claude",
            "codex",
            "a-work",
            "z-work",
            "new-work",
            "traecli",
            "hermes",
            "pi",
            "legacy"
        ]
    );
}

#[tokio::test]
async fn reorder_api_rejects_detected_cross_type_duplicate_and_stale_lists() {
    let state = AppState::new(PathBuf::new());
    let app = crate::build_router(state.clone());
    for id in ["z-work", "a-work"] {
        assert_eq!(
            call(&app, "POST", "/api/aow/agents", profile(id)).await.0,
            StatusCode::CREATED
        );
    }
    let mut other = profile("other");
    other["agent_type"] = json!("claude");
    assert_eq!(
        call(&app, "POST", "/api/aow/agents", other).await.0,
        StatusCode::CREATED
    );
    for ids in [
        json!(["a-work"]),
        json!(["a-work", "a-work"]),
        json!(["other", "a-work"]),
        json!(["codex", "a-work", "z-work"]),
        json!(["deleted", "a-work"]),
    ] {
        assert_eq!(
            call(
                &app,
                "PATCH",
                "/api/aow/agents",
                json!({"agent_type":"codex", "ids":ids})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            state
                .aow
                .lock()
                .unwrap()
                .agents
                .iter()
                .map(|agent| agent.id.as_str())
                .collect::<Vec<_>>(),
            ["other", "z-work", "a-work"]
        );
    }
    assert_eq!(
        call(
            &app,
            "PATCH",
            "/api/aow/agents",
            json!({"agent_type":"codex", "ids":["a-work", "z-work"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, agents) = call(&app, "GET", "/api/aow/agents", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        agents
            .as_array()
            .unwrap()
            .iter()
            .filter(|agent| agent["source"] == "configured")
            .map(|agent| agent["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["other", "a-work", "z-work"]
    );
}

#[tokio::test]
async fn failed_reorder_persistence_keeps_the_previous_in_memory_order() {
    let directory = tempfile::tempdir().unwrap();
    let manager = AowManager::persistent(directory.path()).unwrap();
    for id in ["z-work", "a-work"] {
        manager
            .register_agent(serde_json::from_value(profile(id)).unwrap())
            .await
            .unwrap();
    }
    let registry = manager.inner.agents_path.as_ref().unwrap();
    let before = std::fs::read(registry).unwrap();
    std::fs::remove_file(registry).unwrap();
    std::fs::create_dir(registry).unwrap();
    assert!(
        manager
            .reorder_agents(ReorderAgentsRequest {
                agent_type: Some(AgentType::Codex),
                ids: vec!["a-work".into(), "z-work".into()],
            })
            .await
            .is_err()
    );
    assert_eq!(
        manager
            .lock()
            .unwrap()
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        ["z-work", "a-work"]
    );
    std::fs::remove_dir(registry).unwrap();
    std::fs::write(registry, before).unwrap();
}
