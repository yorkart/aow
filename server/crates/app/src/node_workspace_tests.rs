//! Paths deliberately do not exist on this machine: only the fake node knows them.
use crate::AppState;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::Request,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
#[derive(Clone)]
struct Node {
    root: String,
    calls: Arc<Mutex<Vec<String>>>,
}
async fn operation(State(node): State<Node>, Json(request): Json<Value>) -> Json<Value> {
    let op = request["operation"].as_str().unwrap();
    node.calls.lock().unwrap().push(op.into());
    let args = &request["arguments"];
    Json(match op {
        "canonicalize" => json!({"path":args["path"],"directory":true}),
        "metadata" => json!({"exists":true,"directory":true,"file":false,"symlink":false,"size":0}),
        "mkdir_all" => json!({}),
        "list" => json!({"path":args["path"],"entries":[]}),
        "read" => {
            json!({"path":args["path"],"content":"node-only contents","size":18,"version":"node-version","language":"text","mime":"text/plain"})
        }
        "git_command" => {
            let args = args["args"].as_array().unwrap();
            let text = if args.iter().any(|v| v == "--show-toplevel") {
                format!("{}\n", node.root)
            } else if args.iter().any(|v| v == "--git-common-dir") {
                format!("{}/.git\n", node.root)
            } else if args.first().is_some_and(|v| v == "worktree") {
                format!(
                    "worktree {}\nHEAD 0123456789012345678901234567890123456789\nbranch refs/heads/main\n\n",
                    node.root
                )
            } else {
                String::new()
            };
            json!({"success":true,"code":0,"stdout":text.into_bytes(),"stderr":[]})
        }
        _ => panic!("Unexpected node operation: {request}"),
    })
}
async fn provider(Json(request): Json<Value>) -> Json<Value> {
    Json(
        json!({"version":4,"result":{"name":request["filename"],"hosts":[format!("{}.invalid",request["filename"].as_str().unwrap())],"operations":["repository.commit_links"]}}),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_only_paths_support_project_registration_and_file_apis() {
    let directory = tempfile::tempdir().unwrap();
    let root = format!("/aow-remote-{}/repository", uuid::Uuid::new_v4());
    assert!(!std::path::Path::new(&root).exists());
    let node = Node {
        root: root.clone(),
        calls: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async { Json(json!({"node_id":"remote-node"})) }),
        )
        .route(
            "/v1/environment",
            get(|| async {
                Json(json!({"execution_path":["/node/bin"],"state_dir":"/node/state"}))
            }),
        )
        .route(
            "/v1/environment/discovered-path",
            get(|| async { Json(vec!["/node/discovered/bin"]) }),
        )
        .route("/v1/workspace/provider", post(provider))
        .route("/v1/workspace/operations", post(operation))
        .with_state(node.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let endpoint = aow_runtime_client::RuntimeEndpoint::new(
        &format!("http://{address}"),
        "remote-node-token-0123456789abcdef".into(),
    )
    .unwrap();
    let mut state =
        AppState::with_runtime(Default::default(), directory.path().into(), endpoint).unwrap();
    state.auth = crate::auth::AuthService::disabled();
    let app = crate::build_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/aow/settings/discovered-path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
    let paths: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paths, json!(["/node/discovered/bin"]));

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/aow/projects")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path":root,"notes_path":format!("{root}/notes")}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(status.is_success(), "{body}");
    assert_eq!(body["registered_path"], root);
    assert_eq!(body["worktrees"][0]["path"], root);
    assert!(body["error"].is_null(), "{body}");
    for (path, key, expected) in [
        (format!("/api/fs/tree{root}"), "path", root.clone()),
        (
            format!("/api/fs/text{root}/README.txt"),
            "content",
            "node-only contents".into(),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(response.status().is_success());
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body[key], expected);
    }
    assert!(!std::path::Path::new(&root).exists());
    let calls = node.calls.lock().unwrap();
    for operation in ["canonicalize", "git_command", "mkdir_all", "list", "read"] {
        assert!(calls.iter().any(|call| call == operation), "{calls:?}");
    }
    server.abort();
}

#[tokio::test]
async fn installation_initialization_requires_no_node_and_creates_no_local_workspace() {
    let directory = tempfile::tempdir().unwrap();
    crate::initialize_aow_state(directory.path()).await.unwrap();
    assert!(!directory.path().join("data/repos").exists());
    assert!(!directory.path().join("data/environments").exists());
    assert!(!directory.path().join("data/executions").exists());
    let config = aow_config::ConfigStore::open(directory.path()).unwrap();
    assert!(config.directory().join("git-platform-providers").is_dir());
    crate::initialize_aow_state(directory.path()).await.unwrap();
}
