//! Forks keep a distinct native identity and recover history through the normal load path.
use super::*;

struct ForkGuard {
    threads: Arc<Mutex<ThreadStore>>,
    thread_id: String,
}

impl Drop for ForkGuard {
    fn drop(&mut self) {
        self.threads.lock().unwrap().forking.remove(&self.thread_id);
    }
}

impl AgentServerStore {
    pub async fn fork_session(&self, id: &str) -> Result<SessionSnapshot> {
        let connection = self.session_connection(id)?;
        ensure!(connection.connected(), "ACP adapter is disconnected");
        ensure!(
            connection.supports("fork"),
            "Agent does not support session forking"
        );
        let pending_load = connection.loads.pending_thread(id);
        let (source, reservation) = {
            // The same lock admits prompts, so the idle check and reservation are atomic.
            let mut threads = self.inner.threads.lock().unwrap();
            threads.ensure_session_available(id)?;
            let source = threads.thread(id)?.lock().unwrap().clone();
            ensure!(
                source.status == "idle"
                    && source.active_prompt.is_none()
                    && !source.steering_pending
                    && !source.steering.detached
                    && !source.needs_load
                    && source.permissions.is_empty()
                    && !pending_load,
                "Resume the session and finish the current turn before forking"
            );
            ensure!(
                threads.forking.insert(id.into()),
                "ACP session is being forked"
            );
            (
                source,
                ForkGuard {
                    threads: self.inner.threads.clone(),
                    thread_id: id.into(),
                },
            )
        };
        let session_id = source
            .session_id
            .clone()
            .context("ACP session has not been created")?;
        let service = self.clone();
        // A disappearing HTTP caller must not cancel the RPC and lose the new native identity.
        aow_i18n::spawn(async move {
            let _reservation = reservation;
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
                    thread.title =
                        aow_i18n::message("acp.fork", &[("auto0", source.title.to_string())]);
                    thread.needs_load = true;
                })?;
            service.attach_connection(&connection, &id)?;
            let installed = service.install_session(connection.clone(), result, id.clone());
            let replay = connection.supports("load");
            service
                .inner
                .threads
                .lock()
                .unwrap()
                .update(&id, |thread| {
                    // Seed the local history, including outcomes, before recovery. The normal
                    // replay verifier only carries this metadata over for matching content.
                    // Failed loads retain it for retries, including after a server restart.
                    thread.entries = source.entries;
                    thread.turns = source.turns;
                    thread.terminals = source.terminals;
                    if replay {
                        thread.needs_load = true;
                        thread.status = "disconnected".into();
                    }
                })?;
            if let Err(error) = installed {
                service.record_recovery_error(&id, &error)?;
                return service.snapshot(&id);
            }
            if replay {
                // Fork responses need not replay history (codex-acp does not). Loading the new
                // native session also avoids treating the source's local transcript as authoritative.
                if let Err(error) = service
                    .open_session(connection, fork_id, id.clone(), true, false)
                    .await
                {
                    service.record_recovery_error(&id, &error)?;
                }
            }
            service.snapshot(&id)
        })
        .await
        .context("ACP fork task failed")?
    }
}
