//! Counterpart of thread_import.rs: register metadata without loading a conversation.
use super::AgentServerStore;
use crate::{
    acp_thread::AcpThread,
    facade::{SessionImport, SessionInfo},
};
use anyhow::{Result, ensure};
use std::collections::HashSet;

impl AgentServerStore {
    pub fn import_sessions(
        &self,
        connection_id: &str,
        sessions: Vec<SessionImport>,
    ) -> Result<Vec<SessionInfo>> {
        let connection = self.connection(connection_id)?;
        ensure!(
            connection.supports("list"),
            "Agent does not support session listing"
        );
        // Validate the whole selection before writing any records. AoW archives are workspace scoped.
        for session in &sessions {
            ensure!(
                !session.remote_id.trim().is_empty(),
                "Missing remote session ID"
            );
            ensure!(
                session.cwd == connection.info.cwd,
                "Session belongs to another workspace"
            );
        }
        let mut store = self.inner.threads.lock().unwrap();
        let mut existing: HashSet<_> = store
            .metadata
            .entries()
            .filter(|thread| {
                thread.agent_id == connection.info.agent_id && thread.cwd() == connection.info.cwd
            })
            .filter_map(|thread| thread.session_id.clone())
            .collect();
        let mut imported = Vec::new();
        let mut snapshots = Vec::new();
        for session in sessions {
            if !existing.insert(session.remote_id.clone()) {
                continue;
            }
            let mut thread = AcpThread::new_snapshot(
                uuid::Uuid::new_v4().to_string(),
                session.remote_id,
                connection.info.agent_id.clone(),
                session.cwd,
            );
            thread.status = "disconnected".into();
            thread.needs_load = true;
            if let Some(title) = session.title.filter(|title| !title.trim().is_empty()) {
                thread.title = title;
            }
            if let Some(updated_at) = session
                .updated_at
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            {
                thread.updated_at = updated_at.with_timezone(&chrono::Utc).to_rfc3339();
            }
            let info = SessionInfo::from(&thread);
            snapshots.push(thread);
            imported.push(info);
        }
        store.insert_metadata_all(snapshots)?;
        Ok(imported)
    }
}
