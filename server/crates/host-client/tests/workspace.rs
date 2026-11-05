use aow_host_client::{HostClient, HostError, testing::TestHost};
use aow_i18n::{Language, scope};
use std::path::Path;
#[tokio::test]
async fn request_locale_crosses_json_and_stream_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let node = TestHost::start(&root.path().join("node.json")).await;
    let client = HostClient::new(node.endpoint.clone());
    for (language, expected) in [
        (Language::English, "path must be absolute"),
        (Language::Chinese, "路径必须为绝对路径"),
    ] {
        scope(language, async {
            let HostError::HttpStatus {
                status,
                error: Some(error),
                ..
            } = client
                .workspace_metadata(Path::new("relative"), true)
                .await
                .unwrap_err()
            else {
                panic!("expected structured node error")
            };
            assert_eq!(status, http::StatusCode::BAD_REQUEST);
            assert_eq!(error.code, "path_not_absolute");
            assert_eq!(error.message, expected);
            let response = client
                .workspace_transfer(
                    http::Method::GET,
                    Path::new("relative"),
                    false,
                    false,
                    &http::HeaderMap::new(),
                    None,
                )
                .await
                .unwrap();
            assert_eq!(response.headers()["Content-Language"], language.tag());
            let response: serde_json::Value = response.json().await.unwrap();
            assert_eq!(response["message"], expected);
        })
        .await;
    }
    node.shutdown().await.unwrap();
}
#[tokio::test]
async fn workspace_requests_preserve_typed_file_and_git_results() {
    let root = tempfile::tempdir().unwrap();
    let node = TestHost::start(&root.path().join("node.json")).await;
    let client = HostClient::new(node.endpoint.clone());
    let dir = root.path().join("repo");
    client.workspace_mkdir_all(&dir).await.unwrap();
    let result = client.workspace_git(&dir, &["init", "-q"]).await.unwrap();
    assert!(result.success);
    let created = client
        .workspace_create(
            &dir,
            "note.txt",
            aow_host_contract::workspace::EntryKind::File,
        )
        .await
        .unwrap();
    assert!(
        client
            .workspace_metadata(&created.path, true)
            .await
            .unwrap()
            .file
    );
    assert!(
        client
            .workspace_read(&created.path)
            .await
            .unwrap()
            .content
            .is_empty()
    );
    let renamed = client
        .workspace_rename(&created.path, "renamed.txt", true)
        .await
        .unwrap();
    client
        .workspace_delete(Path::new(&renamed.path))
        .await
        .unwrap();
    assert!(
        !client
            .workspace_metadata(Path::new(&renamed.path), true)
            .await
            .unwrap()
            .exists
    );
    assert_eq!(
        client.workspace_git_status(&dir).await.unwrap().repository,
        dir.to_string_lossy()
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn websocket_upgrade_preserves_the_request_language() {
    use axum::{Router, extract::WebSocketUpgrade, routing::get};
    use futures_util::StreamExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/v1/workspace/watch",
            get(|ws: WebSocketUpgrade| async move {
                aow_i18n::http::upgrade(ws, move |mut socket| async move {
                    socket
                        .send(axum::extract::ws::Message::Text(
                            aow_i18n::current_language().tag().into(),
                        ))
                        .await
                        .unwrap();
                })
            }),
        )
        .layer(axum::middleware::from_fn(aow_i18n::http::middleware));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HostClient::new(
        aow_host_client::HostEndpoint::new(&format!("http://{address}"), "test-token".into())
            .unwrap(),
    );
    for language in [Language::Chinese, Language::English] {
        scope(language, async {
            let mut socket = client.watch_workspaces().await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap().to_text().unwrap(),
                language.tag()
            );
            let _ = socket.close(None).await;
        })
        .await;
    }
    server.abort();
}
