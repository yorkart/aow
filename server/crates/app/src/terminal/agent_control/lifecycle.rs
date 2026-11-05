use super::super::*;
use super::model::info;
use crate::operations::Handle;
use aow_api_contract::terminal::{AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState};

impl TerminalManager {
    pub(in crate::terminal) fn cli_agent(
        &self,
        pane_id: &str,
    ) -> Result<AgentTerminalInfo, TerminalError> {
        let state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        info(&state.tabs[tab], &state.tabs[tab].panes[pane]).ok_or_else(|| {
            TerminalError::Invalid(
                aow_i18n::text("validation.pane_was_not_created_by_agent_create").into(),
            )
        })
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

    pub(in crate::terminal) fn set_agent_state(
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
        agent: &str,
        task: Option<&str>,
        timeout: Duration,
        log: Option<&Handle>,
    ) -> Result<(), TerminalError> {
        let operation = self.agent_operation(pane_id)?;
        let _lease = operation.write_owned().await;
        if let Some(log) = log {
            log.progress(aow_i18n::text("server.wait_for_agent_readiness"), None);
        }
        let result = self
            .run_agent_operation(
                pane_id,
                aow_host_client::agents::TerminalAgentRequest {
                    request_id: format!("initialize-{pane_id}"),
                    operation: aow_host_client::agents::TerminalAgentOperation::Initialize {
                        agent: agent.into(),
                        task: task.map(str::to_owned),
                        timeout_seconds: timeout.as_secs(),
                    },
                },
            )
            .await;
        let progress = match result {
            Ok(()) => {
                if task.is_some()
                    && let Some(log) = log
                {
                    log.progress(aow_i18n::text("server.submit_initial_task"), None);
                }
                AgentTerminalState {
                    phase: AgentTerminalPhase::Ready,
                    error: None,
                    task_submitted: task.is_some(),
                }
            }
            Err(error) => {
                let mut progress = self.cli_agent(pane_id)?.state;
                if matches!(error, TerminalError::DaemonUnavailable(_)) {
                    // A transport failure does not determine the node-owned operation's outcome.
                    // Reconciliation queries its receipt after reconnect, without submitting again.
                    progress.phase = AgentTerminalPhase::Starting;
                    self.inner
                        .restored_initializations
                        .lock()
                        .map_err(|_| TerminalError::Poisoned)?
                        .insert(pane_id.into());
                } else {
                    progress.phase = AgentTerminalPhase::Failed;
                }
                progress.error = Some(error.to_string());
                progress
            }
        };
        self.set_agent_state(pane_id, progress)
    }

    pub(in crate::terminal) async fn run_agent_operation(
        &self,
        pane_id: &str,
        request: aow_host_client::agents::TerminalAgentRequest,
    ) -> Result<(), TerminalError> {
        let progress = self
            .inner
            .terminald
            .run_agent_operation(pane_id, &request)
            .await
            .map_err(map_client_error)?;
        match progress.error {
            Some(error) => Err(TerminalError::Conflict(error)),
            None => Ok(()),
        }
    }
}
