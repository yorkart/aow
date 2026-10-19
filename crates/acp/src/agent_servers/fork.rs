//! Forks keep a distinct native identity and recover history through the normal load path.
use super::*;

impl AgentServerStore {
    pub async fn fork_session(&self, id: &str) -> Result<SessionSnapshot> {
        let source = self.snapshot(id)?;
        let connection = self.session_connection(id)?;
        ensure!(connection.connected(), "ACP adapter is disconnected");
        ensure!(
            connection.supports("fork"),
            "Agent does not support session forking"
        );
        ensure!(
            source.status == "idle"
                && !source.needs_load
                && source.permissions.is_empty()
                && !connection.loads.pending_thread(id),
            "Resume the session and finish the current turn before forking"
        );
        let session_id = source
            .session_id
            .clone()
            .context("ACP session has not been created")?;
        let service = self.clone();
        // A disappearing HTTP caller must not cancel the RPC and lose the new native identity.
        tokio::spawn(async move {
            let result = connection
                .thread_request(
                    &source.thread_id,
                    "session/fork",
                    json!({"sessionId":session_id,"cwd":source.cwd,"mcpServers":[]}),
                )
                .await?;
            let fork_id = result["sessionId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .context("Agent returned no forked session ID")?
                .to_owned();
            ensure!(
                fork_id != session_id,
                "Agent returned the original session ID for a fork"
            );
            ensure!(
                service
                    .inner
                    .threads
                    .lock()
                    .unwrap()
                    .metadata
                    .entry_by_session(&source.agent_id, &source.cwd, &fork_id)
                    .is_none(),
                "Agent returned an existing session ID for a fork"
            );
            let child = service
                .prepare_session_with_options(
                    &source.agent_id,
                    source.cwd.clone().into(),
                    crate::facade::SessionCreation {
                        parent: Some(aow_protocol::InstanceRef::acp(&source.thread_id)),
                        ..Default::default()
                    },
                )
                .await?;
            let id = child.thread_id;
            // Persist identity before decoding configuration or starting a replay, so failures are recoverable.
            service
                .inner
                .threads
                .lock()
                .unwrap()
                .update(&id, |thread| {
                    thread.session_id = Some(fork_id.clone());
                    thread.title = format!("{} · 分叉", source.title);
                    thread.needs_load = true;
                })?;
            service.attach_connection(&connection, &id)?;
            let installed = service.install_session(connection.clone(), result, id.clone());
            if let Err(error) = installed {
                service.record_recovery_error(&id, &error)?;
                return service.snapshot(&id);
            }
            if connection.supports("load") {
                // Fork responses need not replay history (codex-acp does not). Loading the new
                // native session also avoids treating the source's local transcript as authoritative.
                service
                    .inner
                    .threads
                    .lock()
                    .unwrap()
                    .update(&id, |thread| {
                        thread.needs_load = true;
                        thread.status = "disconnected".into();
                    })?;
                if let Err(error) = service
                    .open_session(connection, fork_id, id.clone(), true, false)
                    .await
                {
                    service.record_recovery_error(&id, &error)?;
                }
            } else {
                // Agents without history loading still fork remotely; retain the known local history.
                service
                    .inner
                    .threads
                    .lock()
                    .unwrap()
                    .update(&id, |thread| {
                        thread.entries = source.entries;
                        thread.turns = source.turns;
                        thread.terminals = source.terminals;
                    })?;
            }
            service.snapshot(&id)
        })
        .await
        .context("ACP fork task failed")?
    }
}
