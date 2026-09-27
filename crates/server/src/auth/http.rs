use axum::{
    Json, Router,
    extract::State,
    http::{
        HeaderMap, HeaderValue,
        header::{COOKIE, SET_COOKIE},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use super::{AuthStatus, password};
use crate::{AppState, HttpError};

pub(super) const STATUS_PATH: &str = "/api/auth/status";
pub(super) const LOGIN_PATH: &str = "/api/auth/login";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(STATUS_PATH, get(status))
        .route(LOGIN_PATH, post(login))
}

#[derive(Deserialize)]
struct LoginRequest {
    // Existing clients that omit the method continue to use account/password.
    #[serde(default = "default_method")]
    method: String,
    #[serde(flatten)]
    credentials: Value,
}

fn default_method() -> String {
    password::METHOD.into()
}

async fn status(State(state): State<AppState>, headers: HeaderMap) -> Json<AuthStatus> {
    Json(
        state
            .auth
            .status(session_token(&headers, &state.base_path.cookie_name())),
    )
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, HttpError> {
    let auth = state.auth.clone();
    let token =
        tokio::task::spawn_blocking(move || auth.login(&request.method, request.credentials))
            .await
            .map_err(|_| HttpError::internal("登录验证失败"))??;
    let mut response = Json(state.auth.status(Some(&token))).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={token}; Path={}; HttpOnly; SameSite=Strict",
            state.base_path.cookie_name(),
            state.base_path.url("/"),
        ))
        .expect("UUID session token is a valid cookie value"),
    );
    Ok(response)
}

pub(super) fn session_token<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| {
            cookie
                .split(';')
                .map(str::trim)
                .find_map(|entry| entry.strip_prefix(&format!("{name}=")))
        })
}
