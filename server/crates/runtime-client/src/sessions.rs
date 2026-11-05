//! Native session transport types. The caller never interprets node transcript paths.
use crate::{RuntimeClient, RuntimeError, TerminalClient};
use aow_runtime_contract::agents::{
    NativePaneRequest, NativeProcessCheck, NativeSessionQuery, ResolveSessionWorktree,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub use aow_runtime_contract::agents::{NativePaneSessions, NativeSession, NativeSessionReference};
impl RuntimeClient {
    pub async fn native_sessions(
        &self,
        cwd: &Path,
        agent: Option<&str>,
    ) -> Result<Vec<NativeSession>, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/list",
            &NativeSessionQuery {
                cwd: cwd.into(),
                agent: agent.map(str::to_owned),
                ..Default::default()
            },
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
            &NativeSessionQuery {
                cwd: "/".into(),
                agent: Some(agent.into()),
                session_id: Some(session_id.into()),
                ..Default::default()
            },
        )
        .await
    }
    pub async fn find_native_reference(
        &self,
        reference: &NativeSessionReference,
    ) -> Result<NativeSession, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/find",
            &NativeSessionQuery {
                reference: Some(reference.clone()),
                ..Default::default()
            },
        )
        .await
    }
    pub async fn native_snapshot(
        &self,
        reference: &NativeSessionReference,
    ) -> Result<serde_json::Value, RuntimeError> {
        self.post_json(
            "/v1/agent-sessions/snapshot",
            &NativeSessionQuery {
                reference: Some(reference.clone()),
                ..Default::default()
            },
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
                &ResolveSessionWorktree {
                    cwd: cwd.into(),
                    registered_roots: roots.into(),
                },
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
                &NativePaneRequest {
                    pane_id: pane_id.into(),
                },
            )
            .await
    }
    pub async fn process_current(
        &self,
        pane_id: &str,
        process: &aow_runtime_contract::terminal::TerminalAgentProcess,
    ) -> Result<bool, RuntimeError> {
        #[derive(Deserialize)]
        struct Current {
            current: bool,
        }
        let value: Current = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.runtime().post_json(
                "/v1/agent-sessions/process-check",
                &NativeProcessCheck {
                    pane_id: pane_id.into(),
                    process: process.clone(),
                },
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
