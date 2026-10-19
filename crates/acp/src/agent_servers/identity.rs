//! Resolve a native session only within its live adapter connection.
use super::AgentServerStore;
use anyhow::{Context, Result, ensure};

impl AgentServerStore {
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
