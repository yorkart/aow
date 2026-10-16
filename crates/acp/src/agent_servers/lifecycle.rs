//! AoW prepares durable thread identity before adapter initialization or session/new.
use super::*;

impl AgentServerStore {
    pub async fn prepare_session(&self, agent_id: &str, cwd: PathBuf) -> Result<SessionSnapshot> {
        let cwd = tokio::fs::canonicalize(cwd).await?;
        ensure!(cwd.is_dir(), "ACP working directory must be a directory");
        let settings = validate_settings(&self.inner.host.settings().await?)?;
        ensure!(
            settings.agent_servers.contains_key(agent_id),
            "ACP Agent is not configured; open Settings → ACP"
        );
        let mut thread = AcpThread::new_snapshot(
            uuid::Uuid::new_v4().to_string(),
            String::new(),
            agent_id.into(),
            cwd.to_string_lossy().into_owned(),
        );
        thread.status = "disconnected".into();
        let id = thread.thread_id.clone();
        self.inner.threads.lock().unwrap().insert(thread)?;
        self.snapshot(&id)
    }

    pub fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        self.inner.threads.lock().unwrap().session_info(thread_id)
    }
    pub fn session_connection_info(&self, thread_id: &str) -> Result<Option<ConnectionInfo>> {
        self.session_info(thread_id)?;
        Ok(self
            .session_connection(thread_id)
            .ok()
            .filter(|connection| connection.connected())
            .map(|connection| connection.info.clone()))
    }

    pub(super) fn attach_connection(
        &self,
        connection: &AcpConnection,
        thread_id: &str,
    ) -> Result<()> {
        // Publish the connection during pending creation/replay for request-scoped forms.
        // Detach the previous event source before replacing this association.
        if let Ok(previous) = self.session_connection(thread_id)
            && previous.info.id != connection.info.id
        {
            previous.context.bindings.lock().unwrap().remove(thread_id);
            if let Some(session_id) = self.snapshot(thread_id)?.session_id {
                previous.context.unregister_session(&session_id);
                // Keep quarantined attempts on their original connection.
                // Their late responses must never affect the replacement route.
            }
            if previous.loads.blocks_thread(thread_id)
                && previous.context.bindings.lock().unwrap().is_empty()
            {
                // No other session owns this failed adapter. Shared adapters stay alive.
                previous.close();
            }
        }
        self.inner
            .sessions
            .lock()
            .unwrap()
            .insert(thread_id.into(), connection.info.id.clone());
        connection
            .context
            .bindings
            .lock()
            .unwrap()
            .insert(thread_id.into());
        self.inner
            .events
            .changed(crate::facade::SyncResource::Connection {
                thread_id: thread_id.into(),
            });
        connection.debug_log.attach(thread_id);
        Ok(())
    }

    pub(super) async fn create_on_connection(
        &self,
        connection: Arc<AcpConnection>,
        thread_id: &str,
    ) -> Result<SessionSnapshot> {
        // Authentication errors still need the connection, but have no protocol session yet.
        self.attach_connection(&connection, thread_id)?;
        let result = async {
            let result = connection
                .thread_request(
                    thread_id,
                    "session/new",
                    json!({"cwd":connection.info.cwd,"mcpServers":[]}),
                )
                .await?;
            let session_id = result["sessionId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .context("Agent returned no session ID")?;
            // Retain the remote identity even if decoding modes/configuration subsequently fails.
            self.inner
                .threads
                .lock()
                .unwrap()
                .update(thread_id, |thread| {
                    thread.session_id = Some(session_id.into());
                })?;
            self.register_session(connection, result, thread_id.into())
                .await
        }
        .await;
        if let Err(error) = &result {
            self.record_recovery_error(thread_id, error)?;
        }
        result
    }
}
