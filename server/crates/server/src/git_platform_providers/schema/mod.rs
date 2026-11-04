//! Public, instance-independent contract for uploaded Git platform providers.

use axum::{http::header, response::IntoResponse};

pub(crate) const PUBLIC_PATH: &str = "/api/public/git-platform-provider-schema";
pub(super) const VERSION: u64 = 4;
pub(super) const OPERATIONS: &[&str] = &[
    "pull_requests.list",
    "pull_requests.detail",
    "pull_requests.diff",
    "repository.info",
    "repository.commit_links",
];
pub(super) const DOCUMENT: &str = include_str!("protocol.json");

pub(super) async fn get() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=0, must-revalidate"),
        ],
        DOCUMENT,
    )
}

#[cfg(test)]
mod tests;
