//! Authentication evidence is collected centrally and never includes credentials,
//! cookies, request bodies, query strings, or browser-supplied identity claims as facts.
use std::{convert::Infallible, net::SocketAddr};

use aow_im::{Field, Message};
use aow_operation_log::{Level, Outcome, Record};
use axum::{
    extract::{ConnectInfo, FromRequestParts, Request},
    http::{Extensions, HeaderMap, Method, StatusCode, Uri, Version, request::Parts},
};
use serde::Serialize;

use crate::AppState;

#[derive(Clone, Serialize)]
pub(super) struct Evidence {
    pub event_id: String,
    pub timestamp: String,
    #[serde(skip)]
    started: std::time::Instant,
    method: String,
    path: String,
    http_version: String,
    peer_ip: Option<String>,
    peer_port: Option<u16>,
    host_unverified: Option<String>,
    user_agent: Option<String>,
    origin_unverified: Option<String>,
    referer_unverified: Option<String>,
    x_forwarded_for_unverified: Option<String>,
    x_real_ip_unverified: Option<String>,
    forwarded_unverified: Option<String>,
    session_cookie_present: bool,
}

impl Evidence {
    pub(super) fn from_request(state: &AppState, request: &Request) -> Self {
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
            event_id: uuid::Uuid::new_v4().to_string(),
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
            session_cookie_present: super::http::session_token(
                headers,
                &state.base_path.cookie_name(),
            )
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

pub(super) struct Event {
    pub evidence: Evidence,
    pub kind: &'static str,
    pub event: &'static str,
    pub title: &'static str,
    pub status: StatusCode,
    pub reason: String,
    pub login_method: Option<String>,
    pub claimed_username: Option<String>,
    pub subject: Option<String>,
}

impl Event {
    pub(super) async fn publish(self, state: &AppState) {
        let success = self.event == "login_succeeded";
        let details = serde_json::json!({
            "request": self.evidence,
            "status": self.status.as_u16(),
            "duration_ms": self.evidence.started.elapsed().as_millis(),
            "reason": self.reason,
            "login_method": self.login_method.as_deref().map(|s| clean(s, 64)),
            "claimed_username": self.claimed_username.as_deref().map(|s| clean(s, 256)),
            "subject": self.subject.as_deref().map(|s| clean(s, 256)),
        });
        let record = Record {
            timestamp: self.evidence.timestamp.clone(),
            operation_id: self.evidence.event_id.clone(),
            boot_id: String::new(),
            kind: self.kind.into(),
            source: "auth".into(),
            title: self.title.into(),
            event: self.event.into(),
            level: if success { Level::Info } else { Level::Warn },
            message: details.to_string(),
            project_id: None,
            resource: Some(self.evidence.path.clone()),
            outcome: Some(if success {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            }),
            completed: None,
            total: None,
        };
        tracing::info!(event_id = %record.operation_id, event = self.event, evidence = %record.message, "authentication audit");
        state.operations.record(record.clone()).await;
        // Anonymous access retains its evidence but never enters the IM queue.
        if self.kind != "auth.login" {
            return;
        }
        let field = |label: &str, value: String| Field {
            label: label.into(),
            value,
            url: None,
        };
        let message = Message {
            title: format!("AoW · {}", self.title),
            fields: vec![
                field("时间", self.evidence.timestamp),
                field("事件 ID", self.evidence.event_id),
                field(
                    "连接来源 IP",
                    self.evidence.peer_ip.unwrap_or_else(|| "未知".into()),
                ),
                field(
                    "请求",
                    format!("{} {}", self.evidence.method, self.evidence.path),
                ),
                field(
                    "账号（提交值）",
                    self.claimed_username
                        .as_deref()
                        .map(|s| clean(s, 256))
                        .unwrap_or_else(|| "未提供".into()),
                ),
            ],
            body_label: "审计详情".into(),
            body: format!(
                "HTTP {} · {}\nUser-Agent: {}\n转发 IP（未验证）: {}",
                self.status.as_u16(),
                self.reason,
                self.evidence.user_agent.as_deref().unwrap_or("未知"),
                self.evidence
                    .x_forwarded_for_unverified
                    .as_deref()
                    .unwrap_or("无")
            ),
            markdown: false,
            error: !success,
        };
        state
            .aow
            .notifications()
            .notify_security(message, record, state.operations.clone())
            .await;
    }

    pub(super) fn access(evidence: Evidence, status: StatusCode, reason: &str, page: bool) -> Self {
        Self {
            evidence,
            kind: "auth.access",
            event: if page {
                "anonymous_page_access"
            } else {
                "access_denied"
            },
            title: if page {
                "未登录访问页面"
            } else {
                "未登录访问被拒绝"
            },
            status,
            reason: reason.into(),
            login_method: None,
            claimed_username: None,
            subject: None,
        }
    }
}

pub(super) fn clean(value: &str, max: usize) -> String {
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

fn safe_path(path: &str) -> String {
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

fn safe_url(value: &str) -> Option<String> {
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

#[cfg(test)]
mod tests;
