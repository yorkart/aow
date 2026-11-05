//! Bearer credentials authorize only CLI business routes, never browser administration APIs.
use crate::{AppState, HttpError};
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
    routing::{delete, post},
};
use serde::Deserialize;
pub(crate) async fn authorize(
    state: &AppState,
    request: Request,
    next: Next,
) -> axum::response::Response {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if request.headers().contains_key("origin")
        || token.is_none_or(|token| state.cli_access.authorize(token).is_err())
    {
        return HttpError::new(
            StatusCode::UNAUTHORIZED,
            "cli_authentication_required",
            "A valid Server CLI token is required",
            None,
        )
        .into_response();
    }
    next.run(request).await
}
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cli-tokens", post(issue))
        .route("/api/cli-tokens/{id}", delete(revoke))
}
#[derive(Deserialize)]
struct Issue {
    #[serde(default = "lifetime")]
    ttl_seconds: u64,
}
fn lifetime() -> u64 {
    86400
}
async fn issue(
    State(state): State<AppState>,
    Json(input): Json<Issue>,
) -> Result<Json<aow_cli_access::Grant>, HttpError> {
    state
        .cli_access
        .issue(input.ttl_seconds)
        .map(Json)
        .map_err(|e| {
            HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_cli_token_request",
                e.to_string(),
                None,
            )
        })
}
async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state
        .cli_access
        .revoke(&id)
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
