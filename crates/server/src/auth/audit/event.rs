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
        let message = aow_i18n::with_language(state.aow.notifications().language(), || Message {
            title: format!(
                "AoW · {}",
                aow_i18n::text(if success {
                    "server.login_successful"
                } else {
                    "server.login_failed"
                })
            ),
            fields: vec![
                field(aow_i18n::text("server.time"), self.evidence.timestamp),
                field(aow_i18n::text("server.event_id"), self.evidence.event_id),
                field(
                    aow_i18n::text("server.connection_source_ip"),
                    self.evidence
                        .peer_ip
                        .unwrap_or_else(|| aow_i18n::text("server.unknown").into()),
                ),
                field(
                    aow_i18n::text("server.request"),
                    format!("{} {}", self.evidence.method, self.evidence.path),
                ),
                field(
                    aow_i18n::text("server.username_submitted_value"),
                    self.claimed_username
                        .as_deref()
                        .map(|s| clean(s, 256))
                        .unwrap_or_else(|| aow_i18n::text("server.not_provided").into()),
                ),
            ],
            body_label: aow_i18n::text("server.audit_details").into(),
            body: aow_i18n::message(
                "server.http_user_agent_forwarded_ip_unverified",
                &[
                    ("auto0", format!("{}", self.status.as_u16())),
                    ("auto1", self.reason.to_string()),
                    (
                        "auto2",
                        self.evidence
                            .user_agent
                            .as_deref()
                            .unwrap_or(aow_i18n::text("server.unknown"))
                            .to_string(),
                    ),
                    (
                        "auto3",
                        self.evidence
                            .x_forwarded_for_unverified
                            .as_deref()
                            .unwrap_or(aow_i18n::text("server.none_2"))
                            .to_string(),
                    ),
                ],
            ),
            markdown: false,
            error: !success,
        });
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
                aow_i18n::text("server.unauthenticated_page_access")
            } else {
                aow_i18n::text("server.unauthenticated_access_denied")
            },
            status,
            reason: reason.into(),
            login_method: None,
            claimed_username: None,
            subject: None,
        }
    }
}
