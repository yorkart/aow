use std::{
    collections::HashMap,
    num::NonZeroU32,
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
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, HttpError};

pub(crate) const CREDENTIALS_FILE: &str = "credentials.json";
const PASSWORD_ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();
const SESSION_COOKIE: &str = "aow_session";

#[derive(Clone)]
pub(crate) struct AccountAuth {
    cookie_name: String,
    credentials_path: Option<PathBuf>,
    sessions: Arc<Mutex<HashMap<String, Credentials>>>,
}

enum AccountConfiguration {
    Disabled,
    NotConfigured,
    Configured(Credentials),
    Invalid(String),
}

// Version 1 uses PBKDF2-HMAC-SHA256 (600,000 iterations), a random 16-byte
// salt, and a 32-byte hash. The installer writes the same format atomically.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
struct Credentials {
    version: u32,
    username: String,
    salt: String,
    password_hash: String,
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
    pub username: String,
    pub password: String,
}

impl AccountAuth {
    pub(crate) fn persistent(state_dir: &Path) -> Self {
        Self {
            cookie_name: SESSION_COOKIE.to_owned(),
            credentials_path: Some(state_dir.join(CREDENTIALS_FILE)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// In-memory application states are used by embedders and the existing
    /// route test suite. The server binary always uses `persistent`.
    pub(crate) fn disabled() -> Self {
        Self {
            cookie_name: SESSION_COOKIE.to_owned(),
            credentials_path: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn set_cookie_name(&mut self, name: String) {
        self.cookie_name = name;
    }

    pub(crate) fn status(&self, headers: &HeaderMap) -> AuthStatus {
        match self.configuration() {
            AccountConfiguration::Configured(credentials) => AuthStatus {
                configured: true,
                authenticated: self.has_session(headers, &credentials),
                message: None,
            },
            AccountConfiguration::Disabled => AuthStatus {
                configured: false,
                authenticated: false,
                message: None,
            },
            AccountConfiguration::NotConfigured => AuthStatus {
                configured: false,
                authenticated: false,
                message: Some(
                    "尚未设置登录账户。请在服务器上运行 `aow account` 设置账号和密码。".into(),
                ),
            },
            AccountConfiguration::Invalid(message) => AuthStatus {
                configured: false,
                authenticated: false,
                message: Some(message),
            },
        }
    }

    pub(crate) fn authorize(&self, headers: &HeaderMap) -> Result<(), HttpError> {
        match self.configuration() {
            AccountConfiguration::Disabled => Ok(()),
            AccountConfiguration::Configured(credentials)
                if self.has_session(headers, &credentials) =>
            {
                Ok(())
            }
            AccountConfiguration::Configured(_) => Err(authentication_required()),
            AccountConfiguration::NotConfigured => Err(account_not_configured()),
            AccountConfiguration::Invalid(message) => Err(HttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "account_configuration_invalid",
                message,
                None,
            )),
        }
    }

    pub(crate) fn login(&self, username: &str, password: &str) -> Result<String, HttpError> {
        if !valid_username(username) || password.is_empty() || password.len() > 1024 {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_credentials",
                "请输入有效的账号和密码",
                None,
            ));
        }
        let credentials = match self.configuration() {
            AccountConfiguration::Configured(credentials) => credentials,
            AccountConfiguration::Disabled | AccountConfiguration::NotConfigured => {
                return Err(account_not_configured());
            }
            AccountConfiguration::Invalid(message) => {
                return Err(HttpError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "account_configuration_invalid",
                    message,
                    None,
                ));
            }
        };
        let salt = decode_hex::<16>(&credentials.salt).expect("validated salt");
        let hash = decode_hex::<32>(&credentials.password_hash).expect("validated hash");
        let password_matches = pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            PASSWORD_ITERATIONS,
            &salt,
            password.as_bytes(),
            &hash,
        )
        .is_ok();
        if !password_matches || username != credentials.username {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "账号或密码不正确",
                None,
            ));
        }

        let token = Uuid::new_v4().simple().to_string();
        self.sessions
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?
            .insert(token.clone(), credentials);
        Ok(token)
    }

    fn has_session(&self, headers: &HeaderMap, current_credentials: &Credentials) -> bool {
        let Some(token) = session_token(headers, &self.cookie_name) else {
            return false;
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let Some(session_credentials) = sessions.get(token) else {
            return false;
        };
        if session_credentials == current_credentials {
            true
        } else {
            // Changing the account or password invalidates existing sessions.
            sessions.remove(token);
            false
        }
    }

    fn configuration(&self) -> AccountConfiguration {
        let Some(path) = &self.credentials_path else {
            return AccountConfiguration::Disabled;
        };
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return AccountConfiguration::NotConfigured;
            }
            Err(error) => {
                return AccountConfiguration::Invalid(format!(
                    "无法读取账户配置 {}：{error}",
                    path.display()
                ));
            }
        };
        match serde_json::from_str::<Credentials>(&contents) {
            Ok(credentials)
                if credentials.version == 1
                    && valid_username(&credentials.username)
                    && decode_hex::<16>(&credentials.salt).is_some()
                    && decode_hex::<32>(&credentials.password_hash).is_some() =>
            {
                AccountConfiguration::Configured(credentials)
            }
            _ => AccountConfiguration::Invalid(format!(
                "账户配置 {} 无效；请重新运行 `aow account`",
                path.display()
            )),
        }
    }
}

pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Json<AuthStatus> {
    Json(state.auth.status(&headers))
}

pub(crate) async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, HttpError> {
    let auth = state.auth.clone();
    let token =
        tokio::task::spawn_blocking(move || auth.login(&request.username, &request.password))
            .await
            .map_err(|_| HttpError::internal("登录验证失败"))??;
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
    // Only the registered read-only share route bypasses account authentication.
    // A share token never establishes a AoW login session.
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

fn valid_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 256
        && !username
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

fn account_not_configured() -> HttpError {
    HttpError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "account_not_configured",
        "AoW 尚未设置登录账户；请在服务器上运行 `aow account`",
        None,
    )
}

fn authentication_required() -> HttpError {
    HttpError::new(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "请使用账号和密码登录 AoW",
        None,
    )
}

#[cfg(test)]
pub(crate) fn write_credentials(directory: &Path, username: &str, password: &str) {
    let salt = Uuid::new_v4();
    let mut hash = [0; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        PASSWORD_ITERATIONS,
        salt.as_bytes(),
        password.as_bytes(),
        &mut hash,
    );
    let credentials = Credentials {
        version: 1,
        username: username.into(),
        salt: salt.simple().to_string(),
        password_hash: hash.iter().map(|byte| format!("{byte:02x}")).collect(),
    };
    std::fs::write(
        directory.join(CREDENTIALS_FILE),
        serde_json::to_vec(&credentials).unwrap(),
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn account_and_password_are_both_required_and_password_is_not_normalized() {
        let directory = tempfile::tempdir().unwrap();
        let password = " 密码 p@ss ";
        write_credentials(directory.path(), "本地用户", password);
        let auth = AccountAuth::persistent(directory.path());
        assert!(auth.login("本地用户", password).is_ok());
        for (username, password) in [
            ("", password),
            (" 本地用户", password),
            ("other", password),
            ("本地用户", ""),
            ("本地用户", "123456"),
            ("本地用户", password.trim()),
        ] {
            assert!(auth.login(username, password).is_err());
        }
    }

    #[test]
    fn changed_account_or_password_invalidates_existing_sessions() {
        let directory = tempfile::tempdir().unwrap();
        write_credentials(directory.path(), "admin", "password");
        let auth = AccountAuth::persistent(directory.path());
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            cookie_header(&auth.login("admin", "password").unwrap()),
        );
        assert!(auth.authorize(&headers).is_ok());
        write_credentials(directory.path(), "admin", "new-password");
        assert!(auth.authorize(&headers).is_err());
        headers.insert(
            COOKIE,
            cookie_header(&auth.login("admin", "new-password").unwrap()),
        );
        assert!(auth.authorize(&headers).is_ok());

        // Changing only the username must also revoke sessions, even when the
        // password hash and salt are unchanged.
        let path = directory.path().join(CREDENTIALS_FILE);
        let mut credentials: Credentials =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        credentials.username = "renamed".into();
        fs::write(path, serde_json::to_vec(&credentials).unwrap()).unwrap();
        assert!(auth.authorize(&headers).is_err());
        assert!(auth.login("admin", "new-password").is_err());
        assert!(auth.login("renamed", "new-password").is_ok());
    }

    #[test]
    fn missing_legacy_and_malformed_credentials_never_allow_access() {
        let directory = tempfile::tempdir().unwrap();
        let auth = AccountAuth::persistent(directory.path());
        fs::write(
            directory.path().join("pin.md5"),
            "e10adc3949ba59abbe56e057f20f883e",
        )
        .unwrap();
        assert!(!auth.status(&HeaderMap::new()).configured);
        assert!(auth.authorize(&HeaderMap::new()).is_err());
        assert!(auth.login("admin", "123456").is_err());
        for contents in [
            "",
            "{}",
            "not JSON",
            r#"{"version":1,"username":"admin","salt":"invalid","password_hash":"invalid"}"#,
        ] {
            fs::write(directory.path().join(CREDENTIALS_FILE), contents).unwrap();
            assert!(!auth.status(&HeaderMap::new()).configured);
            assert!(auth.authorize(&HeaderMap::new()).is_err());
            assert!(auth.login("admin", "123456").is_err());
        }
    }
}
