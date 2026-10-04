use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, COOKIE, SET_COOKIE},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use super::{AuthStatus, audit, password};
use crate::{AppState, HttpError};

pub(super) const STATUS_PATH: &str = "/api/auth/status";
pub(super) const LOGIN_PATH: &str = "/api/auth/login";
pub(super) const LOGOUT_PATH: &str = "/api/auth/logout";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(STATUS_PATH, get(status))
        .route(LOGIN_PATH, post(login))
        .route(LOGOUT_PATH, post(logout))
        .layer(axum::middleware::map_response(no_store))
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
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
    evidence: audit::Evidence,
    headers: HeaderMap,
    request: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, HttpError> {
    let mut event = audit::Event {
        evidence,
        kind: "auth.login",
        event: "login_failed",
        title: "登录失败",
        status: StatusCode::BAD_REQUEST,
        reason: String::new(),
        login_method: request
            .as_ref()
            .ok()
            .map(|Json(r)| audit::clean(&r.method, 64)),
        claimed_username: request
            .as_ref()
            .ok()
            .and_then(|Json(r)| r.credentials["username"].as_str())
            .map(|s| audit::clean(s, 256)),
        subject: None,
    };
    // Rejections from JSON extraction (including oversized bodies), origin
    // checks and admission limits are login failures too.
    let admission = check_origin(&headers)
        .and_then(|()| {
            request.map_err(|error| {
                HttpError::new(
                    error.status(),
                    "invalid_login_request",
                    "登录请求格式无效",
                    None,
                )
            })
        })
        .and_then(|Json(request)| state.auth.limiter.begin().map(|attempt| (request, attempt)));
    let (request, attempt) = match admission {
        Ok(admitted) => admitted,
        Err(error) => {
            event.status = error.status;
            event.reason = error.body.code.clone();
            event.publish(&state).await;
            return Err(error);
        }
    };
    // Only admitted attempts create a background task (at most two). Keep the
    // permit until auditing is queued, even when the HTTP caller disconnects.
    tokio::spawn(async move {
        let auth = state.auth.clone();
        let verified = tokio::task::spawn_blocking(move || {
            let result = auth.login_with_attempt(&request.method, request.credentials, &attempt);
            (result, attempt)
        })
        .await;
        let (result, _attempt) = match verified {
            Ok((result, attempt)) => (result, Some(attempt)),
            Err(_) => (Err(HttpError::internal("登录验证失败")), None),
        };
        let result = result.and_then(|token| {
            event.subject = state
                .auth
                .sessions
                .get(&token, false)?
                .map(|session| session.identity.subject);
            login_response(&state, &headers, &token)
        });
        match &result {
            Ok(response) => {
                event.event = "login_succeeded";
                event.title = "登录成功";
                event.status = response.status();
                event.reason = "login_succeeded".into();
            }
            Err(error) => {
                event.status = error.status;
                event.reason = error.body.code.clone();
            }
        }
        event.publish(&state).await;
        result
    })
    .await
    .map_err(|_| HttpError::internal("登录处理失败"))?
}

fn login_response(
    state: &AppState,
    headers: &HeaderMap,
    token: &str,
) -> Result<Response, HttpError> {
    if let Some(previous) = session_token(headers, &state.base_path.cookie_name()) {
        state.auth.sessions.revoke(previous)?;
    }
    let mut response = Json(state.auth.status(Some(token))).into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, cookie(state, token, false));
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, HttpError> {
    check_origin(&headers)?;
    if let Some(token) = session_token(&headers, &state.base_path.cookie_name()) {
        state.auth.sessions.revoke(token)?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, cookie(&state, "", true));
    Ok(response)
}

fn cookie(state: &AppState, token: &str, clear: bool) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}={token}; Path={}; HttpOnly; SameSite=Strict{}{}",
        state.base_path.cookie_name(),
        state.base_path.url("/"),
        if state.auth.secure_cookies {
            "; Secure"
        } else {
            ""
        },
        if clear { "; Max-Age=0" } else { "" },
    ))
    .expect("validated cookie components")
}

// Browser requests must originate from this authority. Do not trust forwarded
// headers supplied by clients, and keep non-browser clients without Origin usable.
pub(crate) fn check_origin(headers: &HeaderMap) -> Result<(), HttpError> {
    if let Some(origin) = headers.get("origin") {
        let valid = origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok())
            .is_some_and(|origin| {
                matches!(origin.scheme_str(), Some("http" | "https"))
                    && origin.path() == "/"
                    && origin.query().is_none()
                    && origin.authority().is_some_and(|authority| {
                        headers
                            .get("host")
                            .and_then(|host| host.to_str().ok())
                            .is_some_and(|host| host.eq_ignore_ascii_case(authority.as_str()))
                    })
            });
        if !valid {
            return Err(HttpError::new(
                StatusCode::FORBIDDEN,
                "invalid_origin",
                "请求来源无效",
                None,
            ));
        }
    }
    Ok(())
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
