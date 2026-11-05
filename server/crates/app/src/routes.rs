//! Server route assembly and shared middleware.

use axum::{
    Router,
    routing::{get, put},
};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::{
    AppState, aow_root, aow_root_redirect, auth, automations, base_path, create_fs_entry,
    delete_fs_entry, fs_path, fs_root, fs_root_redirect, git_commit_detail, git_commit_diff,
    git_commit_files, git_diff, git_ignored, git_log, git_platform_providers, git_pull, git_push,
    git_repositories, git_status, health, help_page, im_api, list_home, list_path, list_root,
    my_pull_request_detail, my_pull_request_diff, my_pull_requests, notifications, operations,
    raw_file, read_text, rename_file_path, rename_fs_entry, root_redirect, session_shares,
    spa_or_asset, terminal, write_file,
};

pub fn build_router(state: AppState) -> Router {
    state.workspace_events.start(state.projects.clone());
    state.collaboration.start(&state);
    let base_path = state.base_path.clone();
    let app = Router::new()
        .merge(auth::routes())
        .merge(crate::cli_access::routes())
        .nest("/api/cli", crate::local_cli::routes())
        .route("/api/health", get(health))
        .route("/api/fs/home", get(list_home))
        .route(
            "/api/fs/search",
            axum::routing::post(crate::filesystem::search::search),
        )
        .route(
            "/api/fs/search/{request_id}",
            axum::routing::delete(crate::filesystem::search::cancel),
        )
        .route("/api/fs/tree", get(list_root))
        .route("/api/fs/tree/{*path}", get(list_path))
        .route("/api/fs/text/{*path}", get(read_text))
        .route("/api/fs/raw/{*path}", get(raw_file))
        .route(
            "/api/fs/file/{*path}",
            put(write_file).patch(rename_file_path),
        )
        .route(
            "/api/fs/entries",
            axum::routing::post(create_fs_entry)
                .patch(rename_fs_entry)
                .delete(delete_fs_entry),
        )
        .route("/api/git/repositories", get(git_repositories))
        .route("/api/git/ignored", axum::routing::post(git_ignored))
        .route("/api/git/status", get(git_status))
        .route("/api/git/pull", axum::routing::post(git_pull))
        .route("/api/git/push", axum::routing::post(git_push))
        .route("/api/git/log", get(git_log))
        .route("/api/git/diff", get(git_diff))
        .route("/api/git/commit/detail", get(git_commit_detail))
        .route("/api/git/commit/files", get(git_commit_files))
        .route("/api/git/commit/diff", get(git_commit_diff))
        .route("/api/my-pull-requests", get(my_pull_requests))
        .route(
            "/api/my-pull-requests/{number}",
            get(my_pull_request_detail),
        )
        .route(
            "/api/my-pull-requests/{number}/diff",
            get(my_pull_request_diff),
        )
        .route("/", get(root_redirect))
        .route("/aow", get(aow_root_redirect))
        .route("/aow/", get(aow_root))
        .route("/aow/tabs/{tab_id}", get(aow_root))
        .route("/aow/tabs/{tab_id}/", get(aow_root))
        .route("/aow/tabs/terminal/{tab_id}", get(aow_root))
        .route("/aow/tabs/acp/{session_id}", get(aow_root))
        .route("/aow/tabs/pr/{provider}/{number}", get(aow_root))
        .route("/aow/tabs/session/{agent}/{session_id}", get(aow_root))
        .route("/aow/tabs/automation/{task_id}", get(aow_root))
        .route(
            "/aow/tabs/automation/{task_id}/runs/{run_id}",
            get(aow_root),
        )
        .route("/m", get(aow_root))
        .route("/m/", get(aow_root))
        .route("/fs", get(fs_root_redirect))
        .route("/fs/", get(fs_root))
        .route("/fs/{*path}", get(fs_path))
        .route("/help", get(help_page))
        .merge(terminal::routes())
        .merge(crate::projects::routes())
        .merge(crate::projects::worktrees::routes())
        .merge(crate::agents::routes())
        .merge(crate::roles::routes())
        .merge(crate::tasks::routes())
        .merge(crate::settings::routes())
        .merge(crate::acp::routes())
        .route("/api/instances", get(crate::instances::list))
        .route("/api/instances/{reference}", get(crate::instances::get))
        .merge(git_platform_providers::routes())
        .merge(notifications::routes())
        .merge(crate::inbox::routes())
        .merge(im_api::routes())
        .merge(operations::routes())
        .merge(crate::realtime::routes())
        .merge(crate::workspace_events::routes())
        .merge(automations::routes())
        .merge(session_shares::routes())
        .fallback(spa_or_asset)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(axum::middleware::from_fn(crate::locale::middleware))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    Router::new()
        .fallback_service(app)
        .layer(axum::middleware::from_fn_with_state(
            base_path,
            base_path::mount,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn business_api_paths_support_mounts_without_legacy_aliases() {
        for base in ["", "/tools/aow"] {
            let directory = tempfile::tempdir().unwrap();
            let mut state = AppState::for_test(directory.path().to_owned());
            state.base_path = crate::BasePath::parse(base).unwrap();
            let app = build_router(state);
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("{base}/api/pinned-worktrees"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let pins: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(pins["paths"], serde_json::json!([]));

            for resource in [
                "projects",
                "agents",
                "agent-sessions",
                "agent-roles",
                "tasks",
                "settings",
                "pinned-worktrees",
                "pinned-directories",
                "worktree-creations",
                "worktree-removals",
                "automations",
                "git-platform-providers",
                "notification-settings",
                "im/wechat",
                "session-shares/missing",
            ] {
                for method in ["GET", "POST", "PATCH"] {
                    let path = format!("{base}/api/aow/{resource}");
                    let response = app
                        .clone()
                        .oneshot(
                            Request::builder()
                                .method(method)
                                .uri(&path)
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(
                        response.status(),
                        axum::http::StatusCode::NOT_FOUND,
                        "{method} {path} must not remain an alias"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn standalone_id_allocation_is_not_exposed() {
        let directory = tempfile::tempdir().unwrap();
        let app = build_router(AppState::for_test(directory.path().to_owned()));
        let response = app
            .clone()
            .oneshot(Request::post("/api/ids").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn untrusted_raw_documents_are_downloaded_and_sandboxed_without_breaking_images() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _node) =
            crate::tests::node_state(directory.path(), directory.path().to_owned()).await;
        let app = build_router(state);
        for (name, attachment) in [
            ("preview.html", true),
            ("preview.svg", true),
            ("preview.xml", true),
            ("preview.js", true),
            ("preview.png", false),
            ("preview.pdf", false),
            ("preview.txt", false),
        ] {
            let path = directory.path().join(name);
            std::fs::write(&path, "test content").unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/api/fs/raw{}", path.display()))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            if attachment {
                assert_eq!(
                    response.headers()["content-security-policy"],
                    "sandbox; default-src 'none'"
                );
            } else {
                assert!(!response.headers().contains_key("content-security-policy"));
            }
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(
                response.headers().contains_key("content-disposition"),
                attachment,
                "{name}"
            );
        }
    }
}
