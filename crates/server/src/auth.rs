use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    extract::{Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, HttpError};

pub(crate) const PIN_HASH_FILE: &str = "pin.md5";
const SESSION_COOKIE: &str = "aow_session";

#[derive(Clone)]
pub(crate) struct PinAuth {
    cookie_name: String,
    pin_hash_path: Option<PathBuf>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
}

enum PinConfiguration {
    Disabled,
    NotConfigured,
    Configured(String),
    Invalid(String),
}

#[derive(Serialize)]
pub(crate) struct AuthStatus {
    pub configured: bool,
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    pub pin: String,
}

impl PinAuth {
    pub(crate) fn persistent(state_dir: &Path) -> Self {
        Self {
            cookie_name: SESSION_COOKIE.to_owned(),
            pin_hash_path: Some(state_dir.join(PIN_HASH_FILE)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// In-memory application states are used by embedders and the existing
    /// route test suite. The server binary always uses `persistent`.
    pub(crate) fn disabled() -> Self {
        Self {
            cookie_name: SESSION_COOKIE.to_owned(),
            pin_hash_path: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn set_cookie_name(&mut self, name: String) {
        self.cookie_name = name;
    }

    pub(crate) fn status(&self, headers: &HeaderMap) -> AuthStatus {
        match self.configuration() {
            PinConfiguration::Configured(hash) => AuthStatus {
                configured: true,
                authenticated: self.has_session(headers, &hash),
                message: None,
            },
            PinConfiguration::Disabled => AuthStatus {
                configured: false,
                authenticated: false,
                message: None,
            },
            PinConfiguration::NotConfigured => AuthStatus {
                configured: false,
                authenticated: false,
                message: Some(format!(
                    "尚未配置 PIN。请在服务器上运行 `aow pin`，它会创建 {PIN_HASH_FILE}。"
                )),
            },
            PinConfiguration::Invalid(message) => AuthStatus {
                configured: false,
                authenticated: false,
                message: Some(message),
            },
        }
    }

    pub(crate) fn authorize(&self, headers: &HeaderMap) -> Result<(), HttpError> {
        match self.configuration() {
            PinConfiguration::Disabled => Ok(()),
            PinConfiguration::Configured(hash) if self.has_session(headers, &hash) => Ok(()),
            PinConfiguration::Configured(_) => Err(authentication_required()),
            PinConfiguration::NotConfigured => Err(pin_not_configured()),
            PinConfiguration::Invalid(message) => Err(HttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "pin_configuration_invalid",
                message,
                None,
            )),
        }
    }

    pub(crate) fn login(&self, pin: &str) -> Result<String, HttpError> {
        if !valid_pin(pin) {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_pin",
                "PIN 必须恰好为 6 位数字",
                None,
            ));
        }
        let expected_hash = match self.configuration() {
            PinConfiguration::Configured(hash) => hash,
            PinConfiguration::Disabled | PinConfiguration::NotConfigured => {
                return Err(pin_not_configured());
            }
            PinConfiguration::Invalid(message) => {
                return Err(HttpError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "pin_configuration_invalid",
                    message,
                    None,
                ));
            }
        };
        let entered_hash = format!("{:x}", md5::compute(pin.as_bytes()));
        if !constant_time_eq(&entered_hash, &expected_hash) {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_pin",
                "PIN 不正确",
                None,
            ));
        }

        let token = Uuid::new_v4().simple().to_string();
        self.sessions
            .lock()
            .map_err(|_| HttpError::internal("PIN 会话锁不可用"))?
            .insert(token.clone(), expected_hash);
        Ok(token)
    }

    fn has_session(&self, headers: &HeaderMap, current_hash: &str) -> bool {
        let Some(token) = session_token(headers, &self.cookie_name) else {
            return false;
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let Some(session_hash) = sessions.get(token) else {
            return false;
        };
        if constant_time_eq(session_hash, current_hash) {
            true
        } else {
            // A newly configured PIN invalidates sessions created with the old one.
            sessions.remove(token);
            false
        }
    }

    fn configuration(&self) -> PinConfiguration {
        let Some(path) = &self.pin_hash_path else {
            return PinConfiguration::Disabled;
        };
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return PinConfiguration::NotConfigured;
            }
            Err(error) => {
                return PinConfiguration::Invalid(format!(
                    "无法读取 PIN 配置 {}：{error}",
                    path.display()
                ));
            }
        };
        let hash = contents.trim();
        if hash.len() != 32 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return PinConfiguration::Invalid(format!(
                "PIN 配置 {} 无效；请重新运行 `aow pin`",
                path.display()
            ));
        }
        PinConfiguration::Configured(hash.to_ascii_lowercase())
    }
}

pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Json<AuthStatus> {
    Json(state.auth.status(&headers))
}

pub(crate) async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, HttpError> {
    let token = state.auth.login(&request.pin)?;
    let mut response = Json(AuthStatus {
        configured: true,
        authenticated: true,
        message: None,
    })
    .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={token}; Path={}; HttpOnly; SameSite=Strict",
            state.auth.cookie_name,
            state.base_path.url("/")
        ))
        .expect("UUID session token is a valid cookie value"),
    );
    Ok(response)
}

pub(crate) async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Only the registered read-only share route bypasses PIN authentication.
    // A share token never establishes a AOW login session.
    let public_share = matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .is_some_and(|path| path.as_str() == crate::session_shares::PUBLIC_API_PATH);
    let protected = (path.starts_with("/api/")
        && !public_share
        && !matches!(path, "/api/health" | "/api/auth/status" | "/api/auth/login"))
        || path == "/fs"
        || path.starts_with("/fs/")
        || path == "/help";
    if protected && let Err(error) = state.auth.authorize(request.headers()) {
        return error.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
pub(crate) fn cookie_header(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}"))
        .expect("UUID session token is a valid cookie value")
}

fn session_token<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
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

fn valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.bytes().all(|byte| byte.is_ascii_digit())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.bytes().zip(right.bytes()) {
        difference |= left ^ right;
    }
    difference == 0
}

fn pin_not_configured() -> HttpError {
    HttpError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "pin_not_configured",
        "AOW 尚未配置 PIN；请在服务器上运行 `aow pin`",
        None,
    )
}

fn authentication_required() -> HttpError {
    HttpError::new(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "需要输入 PIN 码后才能访问 AOW",
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn validates_only_six_digit_pins() {
        assert!(valid_pin("012345"));
        assert!(!valid_pin("12345"));
        assert!(!valid_pin("1234567"));
        assert!(!valid_pin("12a456"));
    }

    #[test]
    fn changed_pin_invalidates_existing_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(PIN_HASH_FILE);
        fs::write(&path, format!("{:x}\n", md5::compute(b"123456"))).unwrap();
        let auth = PinAuth::persistent(directory.path());
        let token = auth.login("123456").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, cookie_header(&token));
        assert!(auth.authorize(&headers).is_ok());

        fs::write(&path, format!("{:x}\n", md5::compute(b"654321"))).unwrap();
        assert!(auth.authorize(&headers).is_err());
    }
}
