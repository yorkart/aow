//! Worktree removal only forgets local history and stops local adapters.
use super::*;

impl AgentServerStore {
    pub fn set_workspace_removing(&self, cwd: &str, removing: bool) {
        self.inner
            .threads
            .lock()
            .unwrap()
            .set_workspace_removing(cwd, removing);
    }

    pub async fn delete_workspace(&self, cwd: &str) -> Result<usize> {
        // Drain any adapter startup before collecting connections. The host has
        // already blocked admission, including pending session preparation.
        let _connect = self.inner.connect_lock.lock().await;
        let connections: Vec<_> = self
            .inner
            .connections
            .lock()
            .unwrap()
            .values()
            .filter(|connection| connection.info.cwd == cwd)
            .cloned()
            .collect();
        // Connections are cwd-scoped, so all their sessions belong to this
        // workspace. Stop the local transport instead of invoking session/delete.
        for connection in &connections {
            connection.close();
        }
        for connection in &connections {
            connection.wait_stopped().await?;
        }
        let mut threads = self.inner.threads.lock().unwrap();
        let ids: Vec<_> = threads
            .sessions(Some(cwd))
            .into_iter()
            .map(|session| session.thread_id)
            .collect();
        for id in &ids {
            threads.remove(id)?;
            self.inner.logs.remove_thread(id);
        }
        // Do not report success while the metadata writer still has pending operations.
        threads.metadata.flush()?;
        drop(threads);
        let connection_ids: std::collections::HashSet<_> = connections
            .iter()
            .map(|connection| connection.info.id.clone())
            .collect();
        for id in &connection_ids {
            self.inner.logs.remove_connection(id);
        }
        self.inner
            .sessions
            .lock()
            .unwrap()
            .retain(|id, connection| !ids.contains(id) && !connection_ids.contains(connection));
        self.inner
            .connections
            .lock()
            .unwrap()
            .retain(|id, _| !connection_ids.contains(id));
        self.inner
            .configurations
            .lock()
            .unwrap()
            .retain(|id, _| !connection_ids.contains(id));
        self.inner
            .progress
            .lock()
            .unwrap()
            .retain(|(_, path), _| path.to_string_lossy() != cwd);
        Ok(ids.len())
    }
}
