use super::*;
use crate::filesystem::encode_absolute;
use axum::{
    body::{Body, to_bytes},
    http::{
        Request, StatusCode,
        header::{CACHE_CONTROL, CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_MATCH, LOCATION, RANGE},
    },
    response::Response,
};
use std::path::Path;
use tempfile::TempDir;
use tower::ServiceExt;

async fn response_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn filesystem_root_and_home_routes_list_directories() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    let app = build_router(AppState::new(frontend));

    let root_listing = app
        .clone()
        .oneshot(Request::get("/api/fs/tree").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(root_listing.status(), StatusCode::OK);
    assert_eq!(response_json(root_listing).await["path"], "/");

    let home_listing = app
        .oneshot(Request::get("/api/fs/home").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(home_listing.status(), StatusCode::OK);
    assert_eq!(
        response_json(home_listing).await["path"],
        filesystem::PROCESS_HOME.to_string_lossy().as_ref()
    );
}

#[tokio::test]
async fn pull_request_routes_are_read_only_and_require_a_repository() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    let app = build_router(AppState::new(frontend));

    for path in [
        "/api/my-pull-requests",
        "/api/my-pull-requests/1",
        "/api/my-pull-requests/1/diff",
    ] {
        let response = app
            .clone()
            .oneshot(Request::post(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }

    let missing_repository = app
        .oneshot(
            Request::get("/api/my-pull-requests")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_repository.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn configured_account_protects_api_routes_and_invalidates_changed_password_sessions() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    let state_dir = root.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    auth::write_credentials(&state_dir, "admin", "test-password");

    let mut state = AppState::new(frontend);
    state.auth = auth::AuthService::persistent(&state_dir);
    let app = build_router(state);

    let denied = app
        .clone()
        .oneshot(Request::get("/api/fs/tree").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    for (payload, expected) in [
        (r#"{"pin":"123456"}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (
            r#"{"method":"unknown","username":"admin","password":"test-password"}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"username":"other","password":"test-password"}"#,
            StatusCode::UNAUTHORIZED,
        ),
        (
            r#"{"username":"admin","password":"wrong-password"}"#,
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(!response.headers().contains_key("set-cookie"));
    }

    let logged_in = app
        .clone()
        .oneshot(
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"admin","password":"test-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logged_in.status(), StatusCode::OK);
    let cookie = logged_in.headers().get("set-cookie").unwrap().clone();

    let allowed = app
        .clone()
        .oneshot(
            Request::get("/api/fs/tree")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);

    auth::write_credentials(&state_dir, "admin", "new-password");
    let invalidated = app
        .oneshot(
            Request::get("/api/fs/tree")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalidated.status(), StatusCode::UNAUTHORIZED);
}

fn git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[tokio::test]
async fn git_sync_routes_pull_and_push_the_requested_worktree() {
    let root = TempDir::new().unwrap();
    let remote = root.path().join("remote.git");
    let source = root.path().join("source");
    let clone = root.path().join("clone");
    let worktree = root.path().join("linked worktree");
    git(
        root.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        root.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            source.to_str().unwrap(),
        ],
    );
    git(&source, &["config", "user.name", "Sync Test"]);
    git(&source, &["config", "user.email", "sync@example.com"]);
    std::fs::write(source.join("file.txt"), "initial\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", "initial"]);
    git(&source, &["push", "-q", "-u", "origin", "main"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "checkout",
            worktree.to_str().unwrap(),
            "origin/main",
        ],
    );
    git(&worktree, &["config", "user.name", "Sync Test"]);
    git(&worktree, &["config", "user.email", "sync@example.com"]);
    git(&worktree, &["config", "push.default", "upstream"]);
    git(&worktree, &["config", "pull.ff", "only"]);

    let app = build_router(AppState::new(root.path().join("frontend")));
    let request = |command: &str, repo: &Path| {
        Request::post(format!("/api/git/{command}"))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::json!({ "repo": repo }).to_string()))
            .unwrap()
    };
    std::fs::write(source.join("file.txt"), "from remote\n").unwrap();
    git(&source, &["commit", "-q", "-am", "remote update"]);
    git(&source, &["push", "-q"]);
    let response = app
        .clone()
        .oneshot(request("pull", &worktree))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read_to_string(worktree.join("file.txt")).unwrap(),
        "from remote\n"
    );
    assert_eq!(
        std::fs::read_to_string(clone.join("file.txt")).unwrap(),
        "initial\n"
    );

    std::fs::write(worktree.join("file.txt"), "from worktree\n").unwrap();
    git(&worktree, &["commit", "-q", "-am", "worktree update"]);
    let response = app
        .clone()
        .oneshot(request("push", &worktree))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    git(&source, &["pull", "-q", "--ff-only"]);
    assert_eq!(
        std::fs::read_to_string(source.join("file.txt")).unwrap(),
        "from worktree\n"
    );

    // A diverged remote must reject an ordinary push, preserving both histories.
    std::fs::write(source.join("file.txt"), "new remote change\n").unwrap();
    git(&source, &["commit", "-q", "-am", "remote advances"]);
    git(&source, &["push", "-q"]);
    std::fs::write(worktree.join("file.txt"), "local change\n").unwrap();
    git(&worktree, &["commit", "-q", "-am", "local advances"]);
    let response = app
        .clone()
        .oneshot(request("push", &worktree))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error = response_json(response).await;
    assert_eq!(error["code"], "git_command_failed");
    assert!(error["message"].as_str().unwrap().contains("rejected"));
    let response = app
        .clone()
        .oneshot(request("pull", &worktree))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    for command in ["pull", "push"] {
        let response = app
            .clone()
            .oneshot(request(command, Path::new("relative")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(response).await["code"], "path_not_absolute");
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/api/git/{command}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}

#[tokio::test]
async fn ignored_route_returns_only_ignored_candidates() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    let repo = root.path().join("ignored repo");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join(".gitignore"), "generated/\n").unwrap();
    std::fs::create_dir(repo.join("generated")).unwrap();
    std::fs::create_dir(repo.join("visible")).unwrap();
    let app = build_router(AppState::new(frontend));
    let body = serde_json::json!({
        "root": repo,
        "paths": [repo.join("generated"), repo.join("visible"), repo.join(".git")]
    });

    let response = app
        .oneshot(
            Request::post("/api/git/ignored")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let response = response_json(response).await;
    assert_eq!(response["repository"], repo.to_string_lossy().as_ref());
    assert_eq!(
        response["ignored"],
        serde_json::json!([repo.join("generated")])
    );
}

#[tokio::test]
async fn commit_detail_route_returns_commit_json() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    let repo = root.path().join("detail repo");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Route Test"]);
    git(&repo, &["config", "user.email", "route@example.com"]);
    std::fs::write(repo.join("file.txt"), "detail\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    git(&repo, &["commit", "-q", "-m", "route detail"]);
    let commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(commit.status.success());
    let commit = String::from_utf8(commit.stdout).unwrap();
    let app = build_router(AppState::new(frontend));
    let repo = repo.to_string_lossy().replace(' ', "%20");
    let response = app
        .oneshot(
            Request::get(format!(
                "/api/git/commit/detail?repo={repo}&commit={}",
                commit.trim()
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["subject"], "route detail");
    assert_eq!(body["stats"]["files_changed"], 1);
    assert_eq!(body["refs"], serde_json::json!(["HEAD", "main"]));
}

#[tokio::test]
async fn router_reads_writes_ranges_and_rejects_stale_versions() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    let file = root.path().join("sample %20.txt");
    std::fs::write(&file, "abcdef").unwrap();
    let encoded = encode_absolute(&file.to_string_lossy());
    let app = build_router(AppState::new(frontend));

    let listing = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/fs/tree{}",
                encode_absolute(&root.path().to_string_lossy())
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listing.status(), StatusCode::OK);
    let listing = response_json(listing).await;
    assert!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "sample %20.txt")
    );

    let read = app
        .clone()
        .oneshot(
            Request::get(format!("/api/fs/text{encoded}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let etag = read.headers()[ETAG].to_str().unwrap().to_owned();
    assert_eq!(response_json(read).await["content"], "abcdef");

    let range = app
        .clone()
        .oneshot(
            Request::get(format!("/api/fs/raw{encoded}"))
                .header(RANGE, "bytes=1-3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.headers()[CONTENT_RANGE], "bytes 1-3/6");
    assert_eq!(&to_bytes(range.into_body(), 16).await.unwrap()[..], b"bcd");

    let write = app
        .clone()
        .oneshot(
            Request::put(format!("/api/fs/file{encoded}"))
                .header(IF_MATCH, &etag)
                .body(Body::from("updated"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(write.status(), StatusCode::OK);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "updated");

    let stale = app
        .clone()
        .oneshot(
            Request::put(format!("/api/fs/file{encoded}"))
                .header(IF_MATCH, etag)
                .body(Body::from("stale"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn upload_route_can_keep_both_files_without_changing_default_writes() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("报告 #1.png");
    std::fs::write(&file, "original").unwrap();
    let app = build_router(AppState::new(root.path().to_path_buf()));
    let encoded = encode_absolute(&file.to_string_lossy());
    let response = app
        .clone()
        .oneshot(
            Request::put(format!("/api/fs/file{encoded}?keep_both=true"))
                .header(CONTENT_TYPE, "image/png")
                .body(Body::from(vec![0, 1, 2, 255]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = response_json(response).await;
    let uploaded = payload["path"].as_str().unwrap();
    assert_eq!(
        uploaded,
        root.path().join("报告 #1 (1).png").to_string_lossy()
    );
    assert_eq!(std::fs::read(uploaded).unwrap(), vec![0, 1, 2, 255]);
    assert_eq!(std::fs::read(&file).unwrap(), b"original");
    let response = app
        .oneshot(
            Request::put(format!("/api/fs/file{encoded}"))
                .body(Body::from("updated"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(std::fs::read(&file).unwrap(), b"updated");
}

#[tokio::test]
async fn rename_route_renames_without_overwriting() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    let source = root.path().join("before.txt");
    let existing = root.path().join("existing.txt");
    std::fs::write(&source, "content").unwrap();
    std::fs::write(&existing, "keep").unwrap();
    let app = build_router(AppState::new(frontend));

    let response = app
        .clone()
        .oneshot(
            Request::patch(format!(
                "/api/fs/file{}",
                encode_absolute(&source.to_string_lossy())
            ))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"name":"after.ts"}"#))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["name"], "after.ts");
    assert_eq!(
        body["path"],
        root.path().join("after.ts").to_string_lossy().as_ref()
    );
    assert!(!source.exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("after.ts")).unwrap(),
        "content"
    );

    let conflict = app
        .clone()
        .oneshot(
            Request::patch(format!(
                "/api/fs/file{}",
                encode_absolute(&root.path().join("after.ts").to_string_lossy())
            ))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"name":"existing.txt"}"#))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(conflict).await["code"], "destination_exists");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");

    let invalid = app
        .oneshot(
            Request::patch(format!(
                "/api/fs/file{}",
                encode_absolute(&root.path().join("after.ts").to_string_lossy())
            ))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"name":"../escape.ts"}"#))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_json(invalid).await["code"], "invalid_file_name");
}

#[tokio::test]
async fn filesystem_entry_routes_create_rename_and_delete_files_and_directories() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let app = build_router(AppState::new(frontend));

    for (name, kind) in [("notes.txt", "file"), ("design", "directory")] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/fs/entries")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "parent": workspace.to_string_lossy(),
                            "name": name,
                            "kind": kind,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response_json(response).await;
        assert_eq!(body["name"], name);
        assert_eq!(body["kind"], kind);
    }
    assert!(workspace.join("notes.txt").is_file());
    assert!(workspace.join("design").is_dir());

    let rename = app
        .clone()
        .oneshot(
            Request::patch("/api/fs/entries")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "path": workspace.join("design").to_string_lossy(),
                        "name": "plans",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rename.status(), StatusCode::OK);
    assert!(workspace.join("plans").is_dir());

    let unsafe_delete = app
        .clone()
        .oneshot(
            Request::delete("/api/fs/entries?path=/tmp/..")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsafe_delete.status(), StatusCode::BAD_REQUEST);

    for path in [workspace.join("notes.txt"), workspace.join("plans")] {
        let response = app
            .clone()
            .oneshot(
                Request::delete(format!("/api/fs/entries?path={}", path.to_string_lossy()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn aow_notes_use_the_shared_filesystem_routes() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    let repository = root.path().join("repo");
    let notes = root.path().join("notes");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::create_dir(&repository).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
    git(&repository, &["init", "-q", "-b", "main"]);
    let app = build_router(AppState::new(frontend));

    let register = app
        .clone()
        .oneshot(
            Request::post("/api/aow/projects")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "path": repository,
                        "notes_path": notes,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register.status(), StatusCode::CREATED);
    let project = response_json(register).await;
    let project_id = project["id"].as_str().unwrap();
    assert_eq!(project["notes_path"], notes.to_string_lossy().as_ref());
    assert!(notes.is_dir());
    assert!(!notes.join(".git").exists());

    let temporary = app
        .clone()
        .oneshot(
            Request::post(format!("/api/aow/projects/{project_id}/notes/temporary"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"extension":"md"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(temporary.status(), StatusCode::CREATED);
    let temporary = response_json(temporary).await;
    let temporary_path = temporary["path"].as_str().unwrap();
    assert!(temporary_path.starts_with(notes.to_string_lossy().as_ref()));
    let temporary_name = Path::new(temporary_path)
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert!(temporary_name.starts_with(".tmp-"));
    assert!(temporary_path.ends_with(".md"));
    assert_eq!(temporary_name.len(), 16);

    let listing = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/fs/tree{}",
                encode_absolute(&notes.to_string_lossy())
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listing.status(), StatusCode::OK);
    let listing = response_json(listing).await;
    assert_eq!(listing["path"], notes.to_string_lossy().as_ref());
    assert!(
        !listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == ".git")
    );
    assert!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == temporary_path)
    );

    let read = app
        .clone()
        .oneshot(
            Request::get(format!("/api/fs/text{}", encode_absolute(temporary_path)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let etag = read.headers()[ETAG].to_str().unwrap().to_owned();

    let write = app
        .clone()
        .oneshot(
            Request::put(format!("/api/fs/file{}", encode_absolute(temporary_path)))
                .header(IF_MATCH, etag)
                .body(Body::from("# Notes\n"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(write.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(temporary_path).unwrap(),
        "# Notes\n"
    );

    let directory = app
        .clone()
        .oneshot(
            Request::post("/api/fs/entries")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "parent": notes,
                        "name": "tasks",
                        "kind": "directory",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(directory.status(), StatusCode::CREATED);

    let file = app
        .clone()
        .oneshot(
            Request::post("/api/fs/entries")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "parent": notes.join("tasks"),
                        "name": "todo.txt",
                        "kind": "file",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(file.status(), StatusCode::CREATED);
    assert_eq!(
        response_json(file).await["path"],
        notes.join("tasks/todo.txt").to_string_lossy().as_ref()
    );

    let rename = app
        .clone()
        .oneshot(
            Request::patch("/api/fs/entries")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "path": notes.join("tasks/todo.txt"),
                        "name": "done.txt",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rename.status(), StatusCode::OK);
    assert_eq!(
        response_json(rename).await["path"],
        notes.join("tasks/done.txt").to_string_lossy().as_ref()
    );

    let delete = app
        .oneshot(
            Request::delete(format!(
                "/api/fs/entries?path={}",
                notes.join("tasks").to_string_lossy()
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);
    assert!(!notes.join("tasks").exists());
}

#[tokio::test]
async fn mobile_entry_is_served_and_ui_selection_survives_redirects() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<div id=root>mobile</div>").unwrap();
    let app = build_router(AppState::new(frontend));
    for path in ["/m", "/m/", "/m?ui=mobile"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            "<div id=root>mobile</div>"
        );
    }
    for path in ["/?ui=desktop", "/aow?ui=desktop", "/?root=/tmp&ui=desktop"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(response.headers()[LOCATION], "/aow/?ui=desktop");
    }
    let response = app
        .oneshot(
            Request::get("/missing-mobile-asset.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn aow_is_served_and_workspace_route_is_not_served() {
    let root = TempDir::new().unwrap();
    let frontend = root.path().join("frontend");
    std::fs::create_dir_all(frontend.join("assets")).unwrap();
    std::fs::write(
        frontend.join("index.html"),
        "<div id=\"root\">workspace</div>",
    )
    .unwrap();
    std::fs::write(frontend.join("assets/index-abc123.js"), "console.log('ok')").unwrap();
    let app = build_router(AppState::new(frontend));
    let root_redirect = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(root_redirect.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(root_redirect.headers()[LOCATION], "/aow/");

    let aow = app
        .clone()
        .oneshot(Request::get("/aow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(aow.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(aow.headers()[LOCATION], "/aow/");

    let aow = app
        .clone()
        .oneshot(Request::get("/aow/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(aow.status(), StatusCode::OK);
    assert_eq!(aow.headers()[CACHE_CONTROL], "no-cache");
    assert!(
        String::from_utf8(to_bytes(aow.into_body(), 1024).await.unwrap().to_vec())
            .unwrap()
            .contains("workspace")
    );

    for path in [
        "/aow/tabs/example",
        "/aow/tabs/example/",
        "/aow/tabs/example?ui=mobile",
        "/aow/tabs/terminal/example",
        "/aow/tabs/files?workspace=w&path=%2Ftmp",
        "/aow/tabs/file?workspace=w&path=%2Ftmp%2Freadme.md",
        "/aow/tabs/diff?workspace=w&source=staged",
        "/aow/tabs/pr/custom/42?workspace=w",
        "/aow/tabs/session/codex/example?workspace=w",
        "/aow/tabs/automation/task?workspace=w",
        "/aow/tabs/automation/task/runs/run?workspace=w",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        assert!(
            String::from_utf8(to_bytes(response.into_body(), 1024).await.unwrap().to_vec())
                .unwrap()
                .contains("workspace")
        );
    }
    let unknown = app
        .clone()
        .oneshot(
            Request::get("/aow/unknown/page")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let legacy_workspace = app
        .clone()
        .oneshot(Request::get("/workspace/tmp/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(legacy_workspace.status(), StatusCode::NOT_FOUND);
    let browser = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/fs{}/",
                encode_absolute(&root.path().to_string_lossy())
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(browser.status(), StatusCode::OK);
    let browser = String::from_utf8(
        to_bytes(browser.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(browser.contains("<th>权限</th>"));
    assert!(browser.contains("Project AoW"));
    assert!(browser.contains("/aow/"));

    let legacy_view = app
        .clone()
        .oneshot(Request::get("/view/tmp/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(legacy_view.status(), StatusCode::NOT_FOUND);

    let legacy_workspace = app
        .clone()
        .oneshot(Request::get("/workspace/tmp").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(legacy_workspace.status(), StatusCode::NOT_FOUND);

    let legacy_query = app
        .clone()
        .oneshot(Request::get("/?root=/tmp").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(legacy_query.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(legacy_query.headers()[LOCATION], "/aow/");

    let asset = app
        .oneshot(
            Request::get("/assets/index-abc123.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
}
