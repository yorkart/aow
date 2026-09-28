use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::time::Duration;
use tower::ServiceExt;

async fn register(state: &AppState, root: &Path) -> String {
    let notes = tempfile::tempdir().unwrap();
    let response = crate::build_router(state.clone())
        .oneshot(
            Request::post("/api/aow/projects")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"path": root, "notes_path": notes.path()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn git(root: &Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Watch Test",
            "-c",
            "user.email=watch@example.invalid",
        ])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-b", "main"]).await;
    git(
        directory.path(),
        &["commit", "--allow-empty", "-m", "initial"],
    )
    .await;
    directory
}

async fn changed(receiver: &mut watch::Receiver<Snapshot>, root: &Path, after: u64) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = receiver.borrow_and_update().clone();
            if snapshot
                .repositories
                .get(root.to_str().unwrap())
                .is_some_and(|revision| *revision > after)
            {
                return snapshot;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .expect("native Git metadata event was not delivered")
}

#[tokio::test]
async fn registered_projects_are_watched_without_clients_and_removal_releases_them() {
    let first = repository().await;
    let second = repository().await;
    let state = AppState::new(first.path().to_owned());
    let first_id = register(&state, first.path()).await;
    let mut receiver = state.workspace_events.0.changes.subscribe();
    changed(&mut receiver, first.path(), 0).await;
    register(&state, second.path()).await;
    let before = changed(&mut receiver, second.path(), 0).await;
    git(
        first.path(),
        &["commit", "--allow-empty", "-m", "first only"],
    )
    .await;
    let after = changed(&mut receiver, first.path(), before.revision).await;
    assert_eq!(before.projects, after.projects);
    assert_eq!(
        before.repositories[second.path().to_str().unwrap()],
        after.repositories[second.path().to_str().unwrap()]
    );
    let response = crate::build_router(state.clone())
        .oneshot(
            Request::delete(format!("/api/aow/projects/{first_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !receiver
                .borrow_and_update()
                .repositories
                .contains_key(first.path().to_str().unwrap())
            {
                break;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stream_starts_with_a_snapshot_and_delivers_terminal_changes_without_git_changes() {
    use futures_util::StreamExt;

    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(directory.path().to_owned());
    let app = routes().with_state(state.clone());
    let response = app
        .oneshot(
            Request::get("/api/workspace/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: workspace"));
    assert!(first.contains("\"terminals\":0"));
    state.workspace_events.terminals_changed();
    let next = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8(next.to_vec())
            .unwrap()
            .contains("\"terminals\":1")
    );
}
