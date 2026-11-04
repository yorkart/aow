//! Browser projections of common native-session records. Node references stay private.
use crate::{RuntimeClient, RuntimeError, TerminalClient};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use aow_runtime_contract::NativeSessionReference;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeSession {
    pub id: String,
    pub agent: String,
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing)]
    pub reference: NativeSessionReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePaneSessions {
    pub agent: String,
    pub cwd: String,
    pub title: String,
    pub process: Option<aow_protocol::TerminalAgentProcess>,
    pub live_session_id: Option<String>,
    pub sessions: Vec<NativeSession>,
    #[serde(skip_serializing)]
    pub current: Option<NativeSessionReference>,
}
impl From<aow_runtime_contract::NativeSessionRecord> for NativeSession {
    fn from(value: aow_runtime_contract::NativeSessionRecord) -> Self {
        Self {
            id: value.id,
            agent: value.agent,
            session_id: value.session_id,
            title: value.title,
            cwd: value.cwd,
            created_at: value.created_at,
            updated_at: value.updated_at,
            reference: value.reference,
        }
    }
}
impl From<aow_runtime_contract::NativePaneSessionRecord> for NativePaneSessions {
    fn from(value: aow_runtime_contract::NativePaneSessionRecord) -> Self {
        Self {
            agent: value.agent,
            cwd: value.cwd,
            title: value.title,
            process: value.process,
            live_session_id: value.live_session_id,
            sessions: value.sessions.into_iter().map(Into::into).collect(),
            current: value.current,
        }
    }
}
impl RuntimeClient {
    pub async fn native_sessions(
        &self,
        cwd: &Path,
        agent: Option<&str>,
    ) -> Result<Vec<NativeSession>, RuntimeError> {
        let records: Vec<aow_runtime_contract::NativeSessionRecord> = self
            .post_json(
                "/v1/agent-sessions/list",
                &serde_json::json!({"cwd":cwd,"agent":agent}),
            )
            .await?;
        Ok(records.into_iter().map(Into::into).collect())
    }
    pub async fn find_native_session(
        &self,
        agent: &str,
        session_id: &str,
    ) -> Result<NativeSession, RuntimeError> {
        self.post_json::<_, aow_runtime_contract::NativeSessionRecord>(
            "/v1/agent-sessions/find",
            &serde_json::json!({"cwd":"/","agent":agent,"session_id":session_id}),
        )
        .await
        .map(Into::into)
    }
    pub async fn find_native_reference(
        &self,
        reference: &NativeSessionReference,
    ) -> Result<NativeSession, RuntimeError> {
        self.post_json::<_, aow_runtime_contract::NativeSessionRecord>(
            "/v1/agent-sessions/find",
            &serde_json::json!({"reference":reference}),
        )
        .await
        .map(Into::into)
    }
    pub async fn native_snapshot(
        &self,
        reference: &NativeSessionReference,
    ) -> Result<serde_json::Value, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/snapshot",
            &serde_json::json!({"reference":reference}),
        )
        .await
    }
    pub async fn resolve_native_worktree(
        &self,
        cwd: &Path,
        roots: &[PathBuf],
    ) -> Result<PathBuf, RuntimeError> {
        #[derive(Deserialize)]
        struct Resolved {
            path: PathBuf,
        }
        let value: Resolved = self
            .post_json(
                "/v1/agent-sessions/resolve-worktree",
                &serde_json::json!({"cwd":cwd,"registered_roots":roots}),
            )
            .await?;
        Ok(value.path)
    }
}
impl TerminalClient {
    pub async fn native_sessions(&self, pane_id: &str) -> Result<NativePaneSessions, RuntimeError> {
        self.runtime()
            .post_json::<_, aow_runtime_contract::NativePaneSessionRecord>(
                "/v1/agent-sessions/pane",
                &serde_json::json!({"pane_id":pane_id}),
            )
            .await
            .map(Into::into)
    }
    pub async fn process_current(
        &self,
        pane_id: &str,
        process: &aow_protocol::TerminalAgentProcess,
    ) -> Result<bool, RuntimeError> {
        #[derive(Deserialize)]
        struct Current {
            current: bool,
        }
        let value: Current = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.runtime().post_json(
                "/v1/agent-sessions/process-check",
                &serde_json::json!({"pane_id":pane_id,"process":process}),
            ),
        )
        .await
        .map_err(|_| {
            RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Node process check timed out",
            ))
        })??;
        Ok(value.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aow_runtime_contract::{NativePaneSessionRecord, NativeSessionRecord};

    #[test]
    fn wire_references_survive_sdk_decoding_but_are_not_exposed_in_browser_views() {
        let record: NativeSessionRecord = serde_json::from_value(serde_json::json!({
            "id":"session", "agent":"codex", "session_id":"native", "title":"Conversation",
            "cwd":"/node/repo", "created_at":"2026-11-04", "updated_at":"2026-11-04",
            "reference":{"node_id":"node", "handle":"private-handle", "agent":"codex",
                "session_id":"native", "cwd":"/node/repo", "title":"Conversation"}
        }))
        .unwrap();
        let pane = NativePaneSessionRecord {
            agent: "codex".into(),
            cwd: "/node/repo".into(),
            title: "Conversation".into(),
            process: None,
            live_session_id: Some("native".into()),
            current: Some(record.reference.clone()),
            sessions: vec![record],
        };
        let wire = serde_json::to_value(&pane).unwrap();
        assert_eq!(wire["current"]["handle"], "private-handle");
        assert_eq!(wire["sessions"][0]["reference"]["handle"], "private-handle");
        let view: NativePaneSessions = pane.into();
        assert_eq!(view.current.as_ref().unwrap().handle, "private-handle");
        assert_eq!(view.sessions[0].reference.handle, "private-handle");
        let public = serde_json::to_value(view).unwrap();
        assert!(public.get("current").is_none());
        assert!(public["sessions"][0].get("reference").is_none());
        assert_eq!(public["sessions"][0]["title"], "Conversation");
    }
}
