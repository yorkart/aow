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

mod authorization;
mod http;
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
            authenticated: !self.disabled && self.authorize(token).is_ok(),
            methods,
            message,
        }
    }

    pub(crate) fn login(&self, method: &str, credentials: Value) -> Result<String, HttpError> {
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

    pub(super) fn authorize(&self, token: Option<&str>) -> Result<(), HttpError> {
        if self.disabled {
            return Ok(());
        }
        if let Some(token) = token
            && let Some(session) = self.sessions.get(token)?
        {
            if let Some(provider) = self
                .providers
                .iter()
                .find(|provider| provider.id() == session.method)
                && provider.validate_session(&session.identity)?
            {
                return Ok(());
            }
            self.sessions.revoke(token)?;
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
