use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::audit;
use super::http::{LOGIN_PATH, LOGOUT_PATH, STATUS_PATH, session_token};
use crate::AppState;
use futures_util::StreamExt;

/// All login methods share this access policy. Only the exact registered
/// read-only share endpoint accepts anonymous API requests with a share token.
pub(crate) async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
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
        && !matches!(path, "/api/health" | STATUS_PATH | LOGIN_PATH | LOGOUT_PATH))
        || path == "/fs"
        || path.starts_with("/fs/")
        || path == "/help";
    let access = if protected {
        match state.auth.access(session_token(
            request.headers(),
            &state.base_path.cookie_name(),
        )) {
            Ok(access) => access,
            Err(error) => {
                audit::Event::access(
                    audit::Evidence::from_request(&state, &request),
                    error.status,
                    &error.body.code,
                    false,
                )
                .publish(&state)
                .await;
                return error.into_response();
            }
        }
    } else {
        None
    };
    // The SPA shell stays public so it can display the login form. Navigating
    // directly to a workspace page still counts as anonymous access.
    let public_page = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .is_some_and(|path| matches!(path.as_str(), "/share/{token}" | "/share/{token}/"));
    let static_asset = path.starts_with("/assets/")
        || path.starts_with("/third-party/")
        || matches!(path, "/workspace-icon.svg" | "/favicon.ico");
    let anonymous_page = !state.auth.disabled
        && !protected
        && !path.starts_with("/api/")
        && !public_page
        && !static_asset
        && matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD
        )
        && state
            .auth
            .check(
                session_token(request.headers(), &state.base_path.cookie_name()),
                false,
            )
            .is_err();
    let evidence = anonymous_page.then(|| audit::Evidence::from_request(&state, &request));
    if let Some(access) = &access {
        request.extensions_mut().insert(access.clone());
    }
    let response = next.run(request).await;
    if let Some(evidence) = evidence {
        audit::Event::access(evidence, response.status(), "authentication_required", true)
            .publish(&state)
            .await;
    }
    // A long-lived event stream must stop when its originating session ends.
    if let Some(access) = access
        && response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        let (parts, body) = response.into_parts();
        let stream = futures_util::stream::unfold(
            (body.into_data_stream(), access),
            |(mut stream, access)| async move {
                if access.validate(false).is_err() {
                    return None;
                }
                tokio::select! {
                    biased;
                    _ = access.revoked() => None,
                    chunk = stream.next() => chunk.map(|chunk| (chunk, (stream, access))),
                }
            },
        );
        return Response::from_parts(parts, Body::from_stream(stream));
    }
    response
}
