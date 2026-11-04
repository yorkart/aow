use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use tokio::process::Command;
#[tokio::test]
async fn remote_commands_use_server_state_and_do_not_fall_back_to_local_storage() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/prefix/api/cli/v1/projects", get(reply))
        .route("/prefix/api/cli/v1/automations", get(reply));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("no-local-state");
    for kind in ["project", "automation"] {
        let output = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
            .args([
                "--server",
                &format!("http://{address}/prefix"),
                "--state-dir",
                missing.to_str().unwrap(),
                kind,
                "list",
            ])
            .env("AOW_SERVER_TOKEN", "remote-test-token")
            .env_remove("AOW_SERVER_TOKEN_FILE")
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            json!({"items":[{"id":"from-server"}]})
        );
    }
    let denied = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .args([
            "--server",
            &format!("http://{address}/prefix"),
            "--state-dir",
            missing.to_str().unwrap(),
            "automation",
            "list",
        ])
        .env("AOW_SERVER_TOKEN", "wrong-token")
        .env_remove("AOW_SERVER_TOKEN_FILE")
        .output()
        .await
        .unwrap();
    assert_eq!(denied.status.code(), Some(4));
    assert!(denied.stdout.is_empty());
    assert!(!missing.exists());
    server.abort();
}
async fn reply(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if headers
        .get("authorization")
        .is_some_and(|value| value == "Bearer remote-test-token")
    {
        Ok(Json(json!({"items":[{"id":"from-server"}]})))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
