//! Request evidence captured and sanitized for authentication auditing.

use std::{convert::Infallible, net::SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request},
    http::{Extensions, HeaderMap, Method, Uri, Version, request::Parts},
};
use serde::Serialize;

use crate::{AppState, auth::http};

#[derive(Clone, Serialize)]
pub(in crate::auth) struct Evidence {
    pub event_id: String,
    pub timestamp: String,
    #[serde(skip)]
    pub started: std::time::Instant,
    pub method: String,
    pub path: String,
    pub http_version: String,
    pub peer_ip: Option<String>,
    pub peer_port: Option<u16>,
    pub host_unverified: Option<String>,
    pub user_agent: Option<String>,
    pub origin_unverified: Option<String>,
    pub referer_unverified: Option<String>,
    pub x_forwarded_for_unverified: Option<String>,
    pub x_real_ip_unverified: Option<String>,
    pub forwarded_unverified: Option<String>,
    pub session_cookie_present: bool,
}

impl Evidence {
    pub(in crate::auth) fn from_request(state: &AppState, request: &Request) -> Self {
        Self::new(
            state,
            request.method(),
            request.uri(),
            request.version(),
            request.headers(),
            request.extensions(),
        )
    }

    fn new(
        state: &AppState,
        method: &Method,
        uri: &Uri,
        version: Version,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> Self {
        let peer = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0);
        let header = |name: &str, limit| {
            headers
                .get(name)
                .and_then(|h| h.to_str().ok())
                .map(|s| clean(s, limit))
        };
        let url_header = |name: &str| {
            headers
                .get(name)
                .and_then(|h| h.to_str().ok())
                .and_then(safe_url)
        };
        Self {
            event_id: aow_id::new_id(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            started: std::time::Instant::now(),
            method: clean(method.as_str(), 32),
            path: safe_path(&state.base_path.url(uri.path())),
            http_version: format!("{version:?}"),
            peer_ip: peer.map(|peer| peer.ip().to_string()),
            peer_port: peer.map(|peer| peer.port()),
            host_unverified: header("host", 256),
            user_agent: header("user-agent", 512),
            origin_unverified: url_header("origin"),
            referer_unverified: url_header("referer"),
            x_forwarded_for_unverified: header("x-forwarded-for", 512),
            x_real_ip_unverified: header("x-real-ip", 128),
            forwarded_unverified: header("forwarded", 512),
            session_cookie_present: http::session_token(headers, &state.base_path.cookie_name())
                .is_some(),
        }
    }
}

impl FromRequestParts<AppState> for Evidence {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self::new(
            state,
            &parts.method,
            &parts.uri,
            parts.version,
            &parts.headers,
            &parts.extensions,
        ))
    }
}

pub(in crate::auth) fn clean(value: &str, max: usize) -> String {
    let mut result: String = value
        .chars()
        .take(max)
        .map(|c| if c.is_control() { '�' } else { c })
        .collect();
    if value.chars().nth(max).is_some() {
        result.push('…');
    }
    result
}

pub(super) fn safe_path(path: &str) -> String {
    // Share URLs contain bearer secrets in their path, including referrers and
    // attempts to use an unsupported verb on the public endpoint.
    let mut redact_next = false;
    let path = path
        .split('/')
        .map(|part| {
            let redact = redact_next;
            redact_next = matches!(part, "share" | "session-shares");
            if redact { "[redacted]" } else { part }
        })
        .collect::<Vec<_>>()
        .join("/");
    clean(&path, 2048)
}

pub(super) fn safe_url(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some(clean(
        &format!(
            "{}{}",
            url.origin().ascii_serialization(),
            safe_path(url.path())
        ),
        1024,
    ))
}
