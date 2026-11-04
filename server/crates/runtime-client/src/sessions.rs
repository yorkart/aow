//! Native session transport types. The caller never interprets node transcript paths.
use crate::{RuntimeClient, RuntimeError, TerminalClient};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSessionReference {
    pub node_id: String,
    pub handle: String,
    pub agent: String,
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: String,
}
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
impl RuntimeClient {
    pub async fn native_sessions(
        &self,
        cwd: &Path,
        agent: Option<&str>,
    ) -> Result<Vec<NativeSession>, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/list",
            &serde_json::json!({"cwd":cwd,"agent":agent}),
        )
        .await
    }
    pub async fn find_native_session(
        &self,
        agent: &str,
        session_id: &str,
    ) -> Result<NativeSession, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/find",
            &serde_json::json!({"cwd":"/","agent":agent,"session_id":session_id}),
        )
        .await
    }
    pub async fn find_native_reference(
        &self,
        reference: &NativeSessionReference,
    ) -> Result<NativeSession, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/find",
            &serde_json::json!({"reference":reference}),
        )
        .await
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
            .post_json(
                "/v1/agent-sessions/pane",
                &serde_json::json!({"pane_id":pane_id}),
            )
            .await
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
