//! Authentication event construction and delivery to operation and security logs.

use aow_im::{Field, Message};
use aow_operation_log::{Level, Outcome, Record};
use axum::http::StatusCode;

use super::{Evidence, clean};
use crate::AppState;

pub(in crate::auth) struct Event {
    pub(in crate::auth) evidence: Evidence,
    pub(in crate::auth) kind: &'static str,
    pub(in crate::auth) event: &'static str,
    pub(in crate::auth) title: &'static str,
    pub(in crate::auth) status: StatusCode,
    pub(in crate::auth) reason: String,
    pub(in crate::auth) login_method: Option<String>,
    pub(in crate::auth) claimed_username: Option<String>,
    pub(in crate::auth) subject: Option<String>,
}

impl Event {
    pub(in crate::auth) async fn publish(self, state: &AppState) {
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

    pub(in crate::auth) fn access(
        evidence: Evidence,
        status: StatusCode,
        reason: &str,
        page: bool,
    ) -> Self {
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
