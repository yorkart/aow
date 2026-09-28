//! Shared authentication, session management, and authorization.
//! Login providers are registered here; business routes do not depend on them.

use std::{path::Path, sync::Arc};

use axum::http::StatusCode;
use serde::Serialize;
use serde_json::Value;

use crate::HttpError;
use password::PasswordProvider;
use provider::LoginProvider;
use sessions::Sessions;

mod audit;
mod authorization;
mod http;
mod limits;
mod password;
mod provider;
mod sessions;

pub(crate) use authorization::require_auth;
pub(crate) use http::routes;
#[cfg(test)]
pub(crate) use password::write_credentials;

#[derive(Clone)]
pub(crate) struct AuthService {
    providers: Vec<Arc<dyn LoginProvider>>,
    sessions: Sessions,
    disabled: bool,
    limiter: limits::LoginLimiter,
    pub(crate) secure_cookies: bool,
}

#[derive(Serialize)]
pub(super) struct LoginMethodStatus {
    id: &'static str,
    label: &'static str,
    configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize)]
pub(super) struct AuthStatus {
    configured: bool,
    authenticated: bool,
    methods: Vec<LoginMethodStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl AuthService {
    pub(crate) fn persistent(state_dir: &Path) -> Self {
        Self::with_providers(vec![Arc::new(PasswordProvider::new(state_dir))])
    }

    fn with_providers(providers: Vec<Arc<dyn LoginProvider>>) -> Self {
        Self {
            providers,
            sessions: Sessions::default(),
            disabled: false,
            limiter: limits::LoginLimiter::default(),
            secure_cookies: false,
        }
    }

    /// Embedders and route fixtures may disable authentication explicitly.
    /// An empty provider registry alone must never disable access checks.
    pub(crate) fn disabled() -> Self {
        Self {
            disabled: true,
            ..Self::with_providers(Vec::new())
        }
    }

    pub(super) fn status(&self, token: Option<&str>) -> AuthStatus {
        let methods: Vec<_> = self
            .providers
            .iter()
            .map(|provider| {
                let error = provider.check_configured().err();
                LoginMethodStatus {
                    id: provider.id(),
                    label: provider.label(),
                    configured: error.is_none(),
                    message: error.map(|error| error.body.message),
                }
            })
            .collect();
        let configured = methods.iter().any(|method| method.configured);
        let message = if configured {
            None
        } else {
            methods.iter().find_map(|method| method.message.clone())
        };
        AuthStatus {
            configured,
            authenticated: !self.disabled && self.check(token, false).is_ok(),
            methods,
            message,
        }
    }

    #[cfg(test)]
    pub(crate) fn login(&self, method: &str, credentials: Value) -> Result<String, HttpError> {
        self.login_with_attempt(method, credentials, &self.limiter.begin()?)
    }

    fn login_with_attempt(
        &self,
        method: &str,
        credentials: Value,
        attempt: &limits::Attempt,
    ) -> Result<String, HttpError> {
        let result = self.authenticate(method, credentials);
        attempt.finish(
            result.is_ok(),
            result
                .as_ref()
                .is_err_and(|error| error.status == StatusCode::UNAUTHORIZED),
        );
        result
    }

    fn authenticate(&self, method: &str, credentials: Value) -> Result<String, HttpError> {
        let provider = self
            .providers
            .iter()
            .find(|provider| provider.id() == method)
            .ok_or_else(|| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_login_method",
                    "不支持此登录方式",
                    None,
                )
            })?;
        let identity = provider.authenticate(&credentials)?;
        self.sessions.create(provider.id(), identity)
    }

    #[cfg(test)]
    pub(super) fn authorize(&self, token: Option<&str>) -> Result<(), HttpError> {
        self.check(token, true).map(|_| ())
    }

    fn access(&self, token: Option<&str>) -> Result<Option<SessionAccess>, HttpError> {
        Ok(self.check(token, true)?.map(|session| SessionAccess {
            auth: self.clone(),
            token: token.expect("authenticated token").to_owned(),
            cancelled: session.cancelled,
        }))
    }

    fn check(
        &self,
        token: Option<&str>,
        touch: bool,
    ) -> Result<Option<sessions::Session>, HttpError> {
        if self.disabled {
            return Ok(None);
        }
        if let Some(token) = token
            && let Some(session) = self.sessions.get(token, false)?
        {
            let validation = self
                .providers
                .iter()
                .find(|provider| provider.id() == session.method)
                .map(|provider| provider.validate_session(&session.identity))
                .unwrap_or(Ok(false));
            match validation {
                Ok(true) => {
                    return self
                        .sessions
                        .get(token, touch)?
                        .map(Some)
                        .ok_or_else(authentication_required);
                }
                result => {
                    self.sessions.revoke(token)?;
                    result?;
                }
            }
        }
        // Report setup errors when none of the login methods is available.
        let mut configuration_error = None;
        for provider in &self.providers {
            match provider.check_configured() {
                Ok(()) => return Err(authentication_required()),
                Err(error) => {
                    configuration_error.get_or_insert(error);
                }
            }
        }
        Err(configuration_error.unwrap_or_else(|| {
            HttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_not_configured",
                "尚未配置可用的登录方式",
                None,
            )
        }))
    }
}

/// A connection retains its originating session; it never becomes an independent authorization.
#[derive(Clone)]
pub(crate) struct SessionAccess {
    auth: AuthService,
    token: String,
    cancelled: tokio_util::sync::CancellationToken,
}

impl SessionAccess {
    pub(crate) fn validate(&self, activity: bool) -> Result<(), HttpError> {
        if self.cancelled.is_cancelled() {
            return Err(authentication_required());
        }
        self.auth.check(Some(&self.token), activity).map(|_| ())
    }

    pub(crate) async fn revoked(&self) {
        loop {
            if self.validate(false).is_err() {
                return;
            }
            tokio::select! {
                _ = self.cancelled.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
            }
        }
    }
}

fn authentication_required() -> HttpError {
    HttpError::new(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "请先登录 AoW",
        None,
    )
}

#[cfg(test)]
mod tests;
