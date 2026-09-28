use super::*;

impl TerminalManager {
    pub(in crate::terminal) async fn create_agent(
        &self,
        request: CreateTerminalRequest,
        launch: AgentLaunch,
    ) -> Result<TerminalTab, TerminalError> {
        self.create_agent_with_state(request, launch, None).await
    }

    pub(in crate::terminal) async fn create_agent_with_state(
        &self,
        request: CreateTerminalRequest,
        mut launch: AgentLaunch,
        agent_terminal: Option<aow_protocol::AgentTerminalState>,
    ) -> Result<TerminalTab, TerminalError> {
        self.ensure_creation_available(&request)?;
        if let Some(session_id) = request.resume_session_id.as_deref() {
            launch
                .resume_session(session_id)
                .map_err(|error| TerminalError::Invalid(error.to_string()))?;
        }
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        if request.shell.is_some() {
            return Err(TerminalError::Invalid(
                "shell cannot be combined with agent_id".to_owned(),
            ));
        }
        let requested_name = request.name.as_deref().map(validate_name).transpose()?;
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())?;
        validate_directory(&cwd).await?;
        let workspace_root = request
            .workspace_root
            .filter(|root| !root.trim().is_empty())
            .unwrap_or_else(|| cwd.clone());
        validate_directory(&workspace_root).await?;
        let (rows, cols) = terminal_size(request.rows, request.cols)?;
        let pane_id = Uuid::new_v4().to_string();
        let tab_id = Uuid::new_v4().to_string();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            name: launch.display_name.clone(),
            cwd,
            shell: launch.executable,
            arguments: launch.args,
            kind: TerminalPaneKind::Agent,
            agent_id: Some(launch.agent_type.id().to_owned()),
            agent_profile_id: request.agent_id,
            agent_terminal,
            restart_on_daemon_restart: false,
            status: TerminalPaneStatus::Running,
            rows,
            cols,
            exit_code: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let tab = {
            let mut state = self.lock_state()?;
            state.ensure_workspace_available(&workspace_root)?;
            state.ensure_workspace_available(&pane.cwd)?;
            let tab = TerminalTab {
                id: tab_id,
                name_is_custom: Some(requested_name.is_some()),
                name: requested_name.unwrap_or_else(|| launch.display_name.clone()),
                workspace_root,
                layout: TerminalLayout::Pane {
                    pane_id: pane_id.clone(),
                },
                panes: vec![pane.clone()],
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            state.tabs.push(tab.clone());
            if let Err(error) = self.persist_locked(&state) {
                state.tabs.pop();
                return Err(error);
            }
            tab
        };
        let mut spec = self.runtime_spec(&pane);
        spec.environment = launch.env;
        if let Err(error) = self
            .inner
            .terminald
            .create(&pane_id, &spec)
            .await
            .map_err(map_create_error)
        {
            self.delete_failed_runtime_best_effort(&pane_id).await;
            self.rollback_created_tab(&tab.id)?;
            return Err(error);
        }
        Ok(tab)
    }
}
