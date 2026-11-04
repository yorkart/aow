use super::*;

impl TerminalManager {
    pub(crate) async fn create_role_agent(
        &self,
        role_id: &str,
        cwd: &str,
        launch: AgentLaunch,
        initial_arguments: &[String],
        managed: bool,
    ) -> Result<TerminalTab, TerminalError> {
        self.create_agent_with_state(
            CreateTerminalRequest {
                name: None,
                cwd: Some(cwd.into()),
                workspace_root: Some(cwd.into()),
                shell: None,
                agent_id: Some(format!("role:{role_id}")),
                resume_session_id: None,
                rows: Some(48),
                cols: Some(160),
            },
            launch,
            initial_arguments,
            managed.then_some(aow_protocol::AgentTerminalState {
                phase: aow_protocol::AgentTerminalPhase::Starting,
                error: None,
                task_submitted: false,
            }),
            None,
            None,
        )
        .await
    }

    pub(crate) fn start_role_agent(
        &self,
        tab: &TerminalTab,
        prompt: Option<String>,
        events: crate::workspace_events::WorkspaceEvents,
    ) -> Result<(), HttpError> {
        let pane = &tab.panes[0];
        if pane.agent_terminal.is_none() && prompt.is_none() {
            return Ok(());
        }
        let adapter = pane
            .agent_id
            .as_deref()
            .and_then(aow_agents::Agent::from_id)
            .and_then(|agent| agent.interactive())
            .ok_or_else(|| {
                crate::instances::invalid("Role runtime does not support interactive startup")
            })?;
        let manager = self.clone();
        let id = pane.id.clone();
        aow_i18n::spawn(async move {
            if let Err(error) = manager
                .initialize_agent_lifecycle(
                    &id,
                    adapter,
                    prompt.as_deref(),
                    Duration::from_secs(120),
                    None,
                )
                .await
            {
                tracing::warn!(%error, pane_id = %id, "Agent role startup failed");
            }
            events.terminals_changed();
        });
        Ok(())
    }
}
