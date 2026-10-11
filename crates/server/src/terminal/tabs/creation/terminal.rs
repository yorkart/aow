use super::*;

impl TerminalManager {
    pub(in crate::terminal) async fn create(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<TerminalTab, TerminalError> {
        self.ensure_creation_available(&request)?;
        if request.resume_session_id.is_some() {
            return Err(TerminalError::Invalid(
                "resume_session_id requires agent_id".to_owned(),
            ));
        }
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        let requested_name = request.name.as_deref().map(validate_name).transpose()?;
        let cwd = resolve_cwd(request.cwd.as_deref(), request.workspace_root.as_deref())?;
        validate_directory(&cwd).await?;
        let workspace_root = request
            .workspace_root
            .filter(|root| !root.trim().is_empty())
            .unwrap_or_else(|| cwd.clone());
        validate_directory(&workspace_root).await?;
        let shell = normalize_shell(request.shell.as_deref())?;
        if request.agent_id.is_some() {
            return Err(TerminalError::Invalid(
                "agent_id is only accepted by the AoW terminal endpoint".to_owned(),
            ));
        }
        let (rows, cols) = terminal_size(request.rows, request.cols)?;
        let pane_id = aow_id::new_id();
        let tab_id = aow_id::new_id();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            parent_pane_id: None,
            name: default_pane_name(&cwd, &shell),
            cwd,
            shell,
            arguments: Vec::new(),
            kind: TerminalPaneKind::Terminal,
            agent_id: None,
            agent_profile_id: None,
            agent_terminal: None,
            hosting: None,
            restart_on_daemon_restart: true,
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
            let name_is_custom = Some(requested_name.is_some());
            let name =
                requested_name.unwrap_or_else(|| format!("Terminal {}", state.tabs.len() + 1));
            let tab = TerminalTab {
                id: tab_id,
                name,
                name_is_custom,
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

        if let Err(error) = self
            .inner
            .terminald
            .create(&pane_id, &self.runtime_spec(&pane))
            .await
            .map_err(map_create_error)
        {
            if self
                .runtime_matches(&pane_id, &self.runtime_spec(&pane))
                .await?
            {
                return Ok(tab);
            }
            self.delete_failed_runtime_best_effort(&pane_id).await;
            self.rollback_created_tab(&tab.id)?;
            return Err(error);
        }
        Ok(tab)
    }
}
