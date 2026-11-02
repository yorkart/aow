//! Resolve a native session only within its live adapter connection.
use super::{AcpConnection, AgentServerStore, ThreadStore};
use anyhow::{Context, Result, ensure};

/// The caller keeps the store locked through any subsequent insertion.
pub(super) fn native_thread_in_environment(
    store: &mut ThreadStore,
    connection: &AcpConnection,
    session_id: &str,
) -> Result<Option<String>> {
    let candidates: Vec<_> = store
        .metadata
        .entries()
        .filter(|thread| {
            thread.agent_id == connection.info.agent_id
                && thread.cwd() == connection.info.cwd
                && thread.session_id.as_deref() == Some(session_id)
        })
        .map(|thread| thread.thread_id.clone())
        .collect();
    for id in candidates {
        let thread = store.thread(&id)?;
        if thread.lock().unwrap().execution_environment == connection.execution_environment {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

impl AgentServerStore {
    pub(super) fn native_thread_in_environment(
        &self,
        connection: &super::AcpConnection,
        session_id: &str,
    ) -> Result<Option<String>> {
        native_thread_in_environment(
            &mut self.inner.threads.lock().unwrap(),
            connection,
            session_id,
        )
    }

    pub fn thread_for_session(&self, connection_id: &str, session_id: &str) -> Result<String> {
        let connection = self.connection(connection_id)?;
        ensure!(
            connection.connected(),
            "ACP caller connection is disconnected"
        );
        let id = connection
            .context
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .context("ACP caller session is not bound to this connection")?;
        ensure!(
            self.session_connection(&id)?.info.id == connection_id,
            "ACP caller session binding has changed"
        );
        Ok(id)
    }
}
