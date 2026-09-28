use std::path::Path;

use axum::{
    Json,
    extract::{Query, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION},
    },
    response::{IntoResponse, Response},
};

use crate::{AppState, HttpError, filesystem::escape_html};

pub(crate) async fn health() -> Json<serde_json::Value> {
    let health =
        serde_json::json!({ "ok": true, "service": "aow", "version": env!("CARGO_PKG_VERSION") });
    #[cfg(target_os = "macos")]
    let health = {
        let mut health = health;
        health["pid"] = std::process::id().into();
        health
    };
    Json(health)
}

pub(crate) async fn root_redirect(
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let location = query
        .get("ui")
        .filter(|mode| matches!(mode.as_str(), "desktop" | "mobile"))
        .map_or_else(|| "/aow/".to_owned(), |mode| format!("/aow/?ui={mode}"));
    (StatusCode::PERMANENT_REDIRECT, [(LOCATION, location)])
}

pub(crate) async fn aow_root_redirect(
    query: Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    root_redirect(query).await
}

pub(crate) async fn aow_root(State(state): State<AppState>) -> Result<Response, HttpError> {
    serve_spa_index(&state).await
}

pub(crate) async fn help_page(State(state): State<AppState>) -> axum::response::Html<String> {
    let mut schema = serde_json::json!({
        "service": "aow",
        "tools": [
            {"name":"list_directory","method":"GET","path":"/api/fs/tree{absolute_path}"},
            {"name":"read_text_file","method":"GET","path":"/api/fs/text{absolute_path}"},
            {"name":"write_file","method":"PUT","path":"/api/fs/file{absolute_path}"},
            {"name":"rename_file","method":"PATCH","path":"/api/fs/file{absolute_path}","body":{"name":"new-name.ext"}},
            {"name":"create_entry","method":"POST","path":"/api/fs/entries","body":{"parent":"/absolute/path","name":"new-entry","kind":"file|directory"}},
            {"name":"rename_entry","method":"PATCH","path":"/api/fs/entries","body":{"path":"/absolute/path","name":"new-name"}},
            {"name":"delete_entry","method":"DELETE","path":"/api/fs/entries?path={absolute_path}"},
            {"name":"discover_repositories","method":"GET","path":"/api/git/repositories?root={absolute_path}"},
            {"name":"git_ignored","method":"POST","path":"/api/git/ignored","body":{"root":"/absolute/explorer/root","paths":["/absolute/explorer/root/entry"]}},
            {"name":"git_status","method":"GET","path":"/api/git/status?repo={absolute_path}"},
            {"name":"git_pull","method":"POST","path":"/api/git/pull","body":{"repo":"/absolute/repository/path"}},
            {"name":"git_push","method":"POST","path":"/api/git/push","body":{"repo":"/absolute/repository/path"}},
            {"name":"git_log","method":"GET","path":"/api/git/log?repo={absolute_path}"},
            {"name":"git_diff","method":"GET","path":"/api/git/diff?repo={absolute_path}&path={relative_path}"}
            ,{"name":"git_commit_detail","method":"GET","path":"/api/git/commit/detail?repo={absolute_path}&commit={commit_id}"}
            ,{"name":"git_commit_files","method":"GET","path":"/api/git/commit/files?repo={absolute_path}&commit={commit_id}"}
            ,{"name":"git_commit_diff","method":"GET","path":"/api/git/commit/diff?repo={absolute_path}&commit={commit_id}&path={relative_path}"}
            ,{"name":"my_pull_requests","method":"GET","path":"/api/my-pull-requests?repo={absolute_path}","description":"Current user's open reviews from the configured provider; add state=all to include merged and closed reviews"}
            ,{"name":"my_pull_request_detail","method":"GET","path":"/api/my-pull-requests/{number}?repo={absolute_path}","description":"Review detail from the configured provider"}
            ,{"name":"my_pull_request_diff","method":"GET","path":"/api/my-pull-requests/{number}/diff?repo={absolute_path}&path={relative_path}","description":"Review file diff from the configured provider"}
        ]
    });
    for tool in schema["tools"].as_array_mut().unwrap() {
        tool["path"] = state.base_path.url(tool["path"].as_str().unwrap()).into();
    }
    axum::response::Html(format!(
        "<!doctype html><meta charset=utf-8><title>AoW API</title><h1>AoW API</h1><pre>{}</pre>",
        escape_html(&serde_json::to_string_pretty(&schema).unwrap())
    ))
}

pub(crate) async fn spa_or_asset(
    State(state): State<AppState>,
    uri: axum::http::Uri,
) -> Result<Response, HttpError> {
    let requested = uri.path().trim_start_matches('/');
    if requested == "index.html" {
        return serve_spa_index(&state).await;
    }
    let safe_asset = !requested.is_empty()
        && Path::new(requested)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    let candidate = state.frontend_dist.join(requested);
    if !safe_asset || !candidate.is_file() {
        return Err(HttpError::new(
            StatusCode::NOT_FOUND,
            "route_not_found",
            "route not found",
            Some(uri.path().to_owned()),
        ));
    }
    let path = candidate;
    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend_not_built",
            format!("frontend asset unavailable at {}: {error}", path.display()),
            None,
        )
    })?;
    let content_type = mime_guess::from_path(&path).first_or_octet_stream();
    let cache_control = if requested.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Ok((
        [
            (CONTENT_TYPE, content_type.as_ref()),
            (CACHE_CONTROL, cache_control),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) async fn serve_spa_index(state: &AppState) -> Result<Response, HttpError> {
    let path = state.frontend_dist.join("index.html");
    let html = tokio::fs::read_to_string(&path).await.map_err(|error| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend_not_built",
            format!("frontend asset unavailable at {}: {error}", path.display()),
            None,
        )
    })?;
    Ok((
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        state.base_path.inject_html(&html),
    )
        .into_response())
}
