use super::super::*;
use super::{connection::AgentConnection, model::info};
use crate::operations::Handle;
use aow_agents::interactive::AgentInteractive;
use aow_protocol::{AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState, TerminalScreen};

impl TerminalManager {
    pub(in crate::terminal) fn cli_agent(
        &self,
        pane_id: &str,
    ) -> Result<AgentTerminalInfo, TerminalError> {
        let state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        info(&state.tabs[tab], &state.tabs[tab].panes[pane])
            .ok_or_else(|| TerminalError::Invalid("pane was not created by agent create".into()))
    }

    pub(in crate::terminal) fn agent_operation(
        &self,
        pane_id: &str,
    ) -> Result<Arc<tokio::sync::RwLock<()>>, TerminalError> {
        let mut operations = self
            .inner
            .agent_operations
            .lock()
            .map_err(|_| TerminalError::Poisoned)?;
        operations.retain(|_, operation| operation.strong_count() > 0);
        if let Some(operation) = operations.get(pane_id).and_then(std::sync::Weak::upgrade) {
            return Ok(operation);
        }
        let operation = Arc::new(tokio::sync::RwLock::new(()));
        operations.insert(pane_id.into(), Arc::downgrade(&operation));
        Ok(operation)
    }

    pub(super) fn set_agent_state(
        &self,
        pane_id: &str,
        agent: AgentTerminalState,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        let old = state.tabs[tab].clone();
        state.tabs[tab].panes[pane].agent_terminal = Some(agent);
        touch_tab(&mut state.tabs[tab]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[tab] = old;
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::terminal) async fn initialize_agent_lifecycle(
        &self,
        pane_id: &str,
        adapter: impl AgentInteractive,
        task: Option<&str>,
        timeout: Duration,
        log: Option<&Handle>,
    ) -> Result<(), TerminalError> {
        let operation = self.agent_operation(pane_id)?;
        let _lease = operation.write_owned().await;
        if let Some(log) = log {
            log.progress("等待 Agent 就绪", None);
        }
        let result =
            tokio::time::timeout(timeout, self.initialize_agent(pane_id, adapter, task, log)).await;
        let progress = match result {
            Ok(Ok(())) => AgentTerminalState {
                phase: AgentTerminalPhase::Ready,
                error: None,
                task_submitted: task.is_some(),
            },
            outcome => {
                let error = match outcome {
                    Ok(Err(error)) => error.to_string(),
                    _ => "agent startup timed out; inspect the pane read-only (startup may require login or trust confirmation)".into(),
                };
                let mut progress = self.cli_agent(pane_id)?.state;
                progress.phase = AgentTerminalPhase::Failed;
                progress.error = Some(error);
                progress
            }
        };
        self.set_agent_state(pane_id, progress)
    }

    pub(super) async fn initialize_agent(
        &self,
        pane_id: &str,
        adapter: impl AgentInteractive,
        task: Option<&str>,
        log: Option<&Handle>,
    ) -> Result<(), TerminalError> {
        let mut connection = AgentConnection::claim(&self.inner.terminald, pane_id).await?;
        self.wait_screen(pane_id, &mut connection, |screen| {
            adapter.input_ready(&screen.lines)
        })
        .await?;
        if let Some(log) = log {
            log.progress("Agent 终端已就绪", None);
        }
        if let Some(task) = task {
            if let Some(log) = log {
                log.progress("提交初始任务", None);
            }
            connection.submit(task).await?;
            let mut progress = self.cli_agent(pane_id)?.state;
            progress.task_submitted = true;
            self.set_agent_state(pane_id, progress)?;
        }
        connection.finish().await
    }

    async fn wait_screen(
        &self,
        pane_id: &str,
        connection: &mut AgentConnection,
        predicate: impl Fn(&TerminalScreen) -> bool,
    ) -> Result<TerminalScreen, TerminalError> {
        let mut interval = tokio::time::interval(Duration::from_millis(150));
        loop {
            tokio::select! {
                message = connection.socket.next() => { AgentConnection::check(message)?; }
                _ = interval.tick() => {
                    if let Some(screen) = self.inner.terminald.screen(pane_id).await.map_err(map_client_error)? {
                        if predicate(&screen) { return Ok(screen); }
                    }
                    let runtime = self.inner.terminald.get(pane_id).await.map_err(map_client_error)?;
                    if runtime.is_none_or(|runtime| runtime.status != TerminalPaneStatus::Running) {
                        return Err(TerminalError::Conflict("agent exited before becoming ready".into()));
                    }
                }
            }
        }
    }
}
