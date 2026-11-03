use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Auth {
    pub token: Arc<str>,
    pub origins: Arc<Vec<String>>,
}

pub(crate) async fn authorize(State(auth): State<Auth>, request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .and_then(|(scheme, token)| scheme.eq_ignore_ascii_case("Bearer").then_some(token));
    if !token.is_some_and(|token| equal(token.as_bytes(), auth.token.as_bytes())) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "Bearer authentication required",
        )
            .into_response();
    }
    // The server is the usual client. Browser access requires an exact configured Origin.
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && !origin
            .to_str()
            .is_ok_and(|origin| auth.origins.iter().any(|allowed| allowed == origin))
    {
        return (StatusCode::FORBIDDEN, "Origin is not allowed").into_response();
    }
    next.run(request).await
}

fn equal(left: &[u8], right: &[u8]) -> bool {
    // Tokens have a fixed public minimum length; compare all bytes for equal-length inputs.
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}
