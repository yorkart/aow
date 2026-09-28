use super::*;

impl TerminalManager {
    pub(in crate::terminal) async fn split(
        &self,
        tab_id: &str,
        request: SplitTerminalRequest,
    ) -> Result<TerminalTab, TerminalError> {
        self.reconcile().await?;
        let _operation = self.inner.operation.lock().await;
        let (target, target_revision) = {
            let state = self.lock_state()?;
            let tab = state
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
            if tab.panes.len() >= MAX_PANES_PER_TAB {
                return Err(TerminalError::Invalid(format!(
                    "a tab may contain at most {MAX_PANES_PER_TAB} panes"
                )));
            }
            let target = tab
                .panes
                .iter()
                .find(|pane| pane.id == request.target_pane_id)
                .cloned()
                .ok_or_else(|| TerminalError::PaneNotFound(request.target_pane_id.clone()))?;
            (target, tab.revision)
        };
        let ratio = validate_ratio(request.ratio.unwrap_or(0.5))?;
        let cwd = request.cwd.unwrap_or_else(|| target.cwd.clone());
        validate_directory(&cwd).await?;
        let inherited_shell =
            (target.kind == TerminalPaneKind::Terminal).then_some(target.shell.as_str());
        let shell = normalize_shell(request.shell.as_deref().or(inherited_shell))?;
        let (rows, cols) = terminal_size(
            request.rows.or(Some(target.rows)),
            request.cols.or(Some(target.cols)),
        )?;
        let pane_id = Uuid::new_v4().to_string();
        let now = timestamp();
        let pane = TerminalPane {
            id: pane_id.clone(),
            name: default_pane_name(&cwd, &shell),
            cwd,
            shell,
            arguments: Vec::new(),
            kind: TerminalPaneKind::Terminal,
            agent_id: None,
            agent_profile_id: None,
            agent_terminal: None,
            restart_on_daemon_restart: true,
            status: TerminalPaneStatus::Running,
            rows,
            cols,
            exit_code: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let (tab, previous_tab) = {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, tab_id)?;
            state.ensure_workspace_available(&state.tabs[index].workspace_root)?;
            state.ensure_workspace_available(&pane.cwd)?;
            if state.tabs[index].revision != target_revision {
                return Err(TerminalError::Conflict(
                    "terminal layout changed while the new pane was being prepared".to_owned(),
                ));
            }
            let old = state.tabs[index].clone();
            let replacement = TerminalLayout::Split {
                axis: request.axis,
                ratio,
                first: Box::new(TerminalLayout::Pane {
                    pane_id: request.target_pane_id.clone(),
                }),
                second: Box::new(TerminalLayout::Pane {
                    pane_id: pane_id.clone(),
                }),
            };
            if !replace_layout_leaf(
                &mut state.tabs[index].layout,
                &request.target_pane_id,
                &replacement,
            ) {
                return Err(TerminalError::Conflict(
                    "the target pane changed while the new pane was being prepared".to_owned(),
                ));
            }
            state.tabs[index].panes.push(pane.clone());
            touch_tab(&mut state.tabs[index]);
            if let Err(error) = self.persist_locked(&state) {
                state.tabs[index] = old;
                return Err(error);
            }
            (state.tabs[index].clone(), old)
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
            self.rollback_split(tab_id, &pane_id, &tab, previous_tab)?;
            return Err(error);
        }
        Ok(tab)
    }

    pub(in crate::terminal) fn rollback_split(
        &self,
        tab_id: &str,
        pane_id: &str,
        created_tab: &TerminalTab,
        previous_tab: TerminalTab,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let index = tab_index(&state, tab_id)?;
        let failed_tab = state.tabs[index].clone();
        if failed_tab.revision == created_tab.revision {
            let mut restored = previous_tab;
            // Preserve runtime metadata that can change without incrementing
            // the structural revision while terminald create is in flight.
            for pane in &mut restored.panes {
                if let Some(current) = failed_tab.panes.iter().find(|item| item.id == pane.id) {
                    pane.status = current.status;
                    pane.rows = current.rows;
                    pane.cols = current.cols;
                    pane.exit_code = current.exit_code;
                    pane.updated_at.clone_from(&current.updated_at);
                }
            }
            if failed_tab.updated_at != created_tab.updated_at {
                restored.updated_at.clone_from(&failed_tab.updated_at);
            }
            state.tabs[index] = restored;
        } else {
            // A concurrent rename/layout update committed. Remove only the
            // failed pane from the latest tab instead of overwriting it with
            // the stale pre-split snapshot.
            state.tabs[index].layout =
                remove_layout_leaf(state.tabs[index].layout.clone(), pane_id).ok_or_else(|| {
                    TerminalError::Conflict(
                        "failed terminal pane disappeared during split rollback".to_owned(),
                    )
                })?;
            state.tabs[index].panes.retain(|pane| pane.id != pane_id);
            touch_tab(&mut state.tabs[index]);
        }
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[index] = failed_tab;
            return Err(error);
        }
        Ok(())
    }
}
