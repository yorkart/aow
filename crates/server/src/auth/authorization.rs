use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::http::{LOGIN_PATH, STATUS_PATH, session_token};
use crate::AppState;

/// All login methods share this access policy. Only the exact registered
/// read-only share endpoint accepts anonymous API requests with a share token.
pub(crate) async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let public_share = matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .is_some_and(|path| path.as_str() == crate::session_shares::PUBLIC_API_PATH);
    let protected = (path.starts_with("/api/")
        && !public_share
        && !matches!(path, "/api/health" | STATUS_PATH | LOGIN_PATH))
        || path == "/fs"
        || path.starts_with("/fs/")
        || path == "/help";
    if protected
        && let Err(error) = state.auth.authorize(session_token(
            request.headers(),
            &state.base_path.cookie_name(),
        ))
    {
        return error.into_response();
    }
    next.run(request).await
}
