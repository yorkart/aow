use super::*;
use aow_agents::Agent;
use aow_protocol::{AgentTerminalPhase, AgentTerminalState};

pub(super) async fn rebuild_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<Json<TerminalTab>, HttpError> {
    // Finish committing or rolling back even if the browser disconnects.
    tokio::spawn(async move {
        let previous = state
            .terminals
            .get(&tab_id)
            .await
            .map_err(terminal_http_error)?;
        ensure_rebuildable(&previous).map_err(terminal_http_error)?;
        let mut specs = Vec::new();
        for pane in &previous.panes {
            let mut spec = runtime_spec(pane);
            if pane.kind == TerminalPaneKind::Agent {
                let agent_id = pane.agent_id.as_deref().ok_or_else(|| {
                    terminal_http_error(TerminalError::Invalid(
                        "终端缺少 Agent 配置，无法重建".into(),
                    ))
                })?;
                if pane.agent_terminal.is_some()
                    && Agent::from_id(agent_id)
                        .and_then(Agent::interactive)
                        .is_none()
                {
                    return Err(terminal_http_error(TerminalError::Invalid(
                        "该 Agent 不支持交互式初始化".into(),
                    )));
                }
                // Credentials and PATH come from the current workspace profile;
                // keep the saved command/arguments (including explicit resume).
                spec.environment = state
                    .aow
                    .resolve_terminal_rebuild_launch(pane, &previous.workspace_root)
                    .await
                    .map_err(crate::aow::aow_http_error)?
                    .env;
            }
            specs.push(spec);
        }
        let tab = state
            .terminals
            .rebuild(&previous, &specs)
            .await
            .map_err(terminal_http_error)?;
        for pane in &tab.panes {
            if pane.agent_terminal.is_some() {
                let adapter = pane
                    .agent_id
                    .as_deref()
                    .and_then(Agent::from_id)
                    .and_then(Agent::interactive)
                    .expect("validated interactive adapter");
                let manager = state.terminals.clone();
                let pane_id = pane.id.clone();
                tokio::spawn(async move {
                    if let Err(error) = manager
                        .initialize_agent_lifecycle(
                            &pane_id,
                            adapter,
                            None,
                            Duration::from_secs(120),
                            None,
                        )
                        .await
                    {
                        tracing::warn!(%pane_id, %error, "failed to initialize rebuilt agent");
                    }
                });
            }
        }
        Ok(Json(tab))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}

fn ensure_rebuildable(tab: &TerminalTab) -> Result<(), TerminalError> {
    if tab.panes.is_empty()
        || tab
            .panes
            .iter()
            .any(|pane| pane.status == TerminalPaneStatus::Running)
    {
        return Err(TerminalError::Conflict(
            "仅已退出或已中断的终端可以重建，请刷新列表".into(),
        ));
    }
    Ok(())
}

impl TerminalManager {
    async fn rebuild(
        &self,
        previous: &TerminalTab,
        specs: &[TerminalRuntimeSpec],
    ) -> Result<TerminalTab, TerminalError> {
        let _operation = self.inner.operation.lock().await;
        let current = self.get_snapshot(&previous.id)?;
        ensure_rebuildable(&current)?;
        if current
            .panes
            .iter()
            .map(|pane| &pane.id)
            .ne(previous.panes.iter().map(|pane| &pane.id))
        {
            return Err(TerminalError::Conflict("终端已发生变化，请刷新列表".into()));
        }
        {
            let state = self.lock_state()?;
            state.ensure_workspace_available(&current.workspace_root)?;
            for pane in &current.panes {
                state.ensure_workspace_available(&pane.cwd)?;
            }
        }
        validate_directory(&current.workspace_root).await?;
        for pane in &current.panes {
            validate_directory(&pane.cwd).await?;
            if self
                .inner
                .terminald
                .get(&pane.id)
                .await
                .map_err(map_client_error)?
                .is_some_and(|runtime| runtime.status == TerminalPaneStatus::Running)
            {
                return Err(TerminalError::Conflict("终端仍在运行，请刷新列表".into()));
            }
        }

        let now = timestamp();
        let panes: Vec<_> = current
            .panes
            .iter()
            .map(|pane| TerminalPane {
                id: Uuid::new_v4().to_string(),
                status: TerminalPaneStatus::Running,
                exit_code: None,
                agent_terminal: pane.agent_terminal.as_ref().map(|_| AgentTerminalState {
                    phase: AgentTerminalPhase::Starting,
                    error: None,
                    task_submitted: false,
                }),
                created_at: now.clone(),
                updated_at: now.clone(),
                ..pane.clone()
            })
            .collect();
        for (index, (pane, spec)) in panes.iter().zip(specs).enumerate() {
            if let Err(error) = self
                .inner
                .terminald
                .create(&pane.id, spec)
                .await
                .map_err(map_create_error)
            {
                for created in &panes[..=index] {
                    self.delete_failed_runtime_best_effort(&created.id).await;
                }
                return Err(error);
            }
        }
        // Keep the old metadata/runtime until every new pane has started. New
        // pane IDs also isolate stale WebSocket exit events and CLI leases.
        let result = (|| {
            let mut state = self.lock_state()?;
            let index = tab_index(&state, &current.id)?;
            let old = state.tabs[index].clone();
            let mut rebuilt = old.clone();
            for (before, after) in current.panes.iter().zip(&panes) {
                replace_layout_leaf(
                    &mut rebuilt.layout,
                    &before.id,
                    &TerminalLayout::Pane {
                        pane_id: after.id.clone(),
                    },
                );
            }
            rebuilt.panes = panes.clone();
            touch_tab(&mut rebuilt);
            state.tabs[index] = rebuilt.clone();
            if let Err(error) = self.persist_locked(&state) {
                state.tabs[index] = old;
                return Err(error);
            }
            Ok(rebuilt)
        })();
        if result.is_err() {
            for pane in &panes {
                self.delete_failed_runtime_best_effort(&pane.id).await;
            }
        } else {
            for pane in &current.panes {
                self.delete_failed_runtime_best_effort(&pane.id).await;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{pane, start_daemon, stop_daemon, tab_with};
    use super::*;

    #[tokio::test]
    async fn rebuild_replaces_finished_panes_preserving_tab_and_layout() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("terminald/daemon.sock");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket);
        let manager =
            TerminalManager::persistent(directory.path().join("state"), client.clone()).unwrap();
        let mut exited = pane("exited", TerminalPaneStatus::Exited);
        exited.exit_code = Some(9);
        exited.restart_on_daemon_restart = false;
        let mut interrupted = pane("interrupted", TerminalPaneStatus::Interrupted);
        interrupted.kind = TerminalPaneKind::Agent;
        interrupted.agent_id = Some("codex".into());
        interrupted.restart_on_daemon_restart = false;
        interrupted.arguments = vec![
            "-c".into(),
            "printf '%s' \"$REBUILD_FIXTURE\" > \"$REBUILD_OUTPUT\"; exec sleep 60".into(),
        ];
        let old = tab_with(
            TerminalLayout::Split {
                axis: TerminalSplitAxis::Row,
                ratio: 0.4,
                first: Box::new(TerminalLayout::Pane {
                    pane_id: exited.id.clone(),
                }),
                second: Box::new(TerminalLayout::Pane {
                    pane_id: interrupted.id.clone(),
                }),
            },
            vec![exited, interrupted],
        );
        let mut exit_spec = runtime_spec(&old.panes[0]);
        exit_spec.arguments = vec!["-c".into(), "exit 9".into()];
        client.create("exited", &exit_spec).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while client.get("exited").await.unwrap().unwrap().status == TerminalPaneStatus::Running
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        manager.lock_state().unwrap().tabs.push(old.clone());
        let mut specs: Vec<_> = old.panes.iter().map(runtime_spec).collect();
        specs[1]
            .environment
            .insert("REBUILD_FIXTURE".into(), "preserved".into());
        let output = directory.path().join("environment.txt");
        specs[1].environment.insert(
            "REBUILD_OUTPUT".into(),
            output.to_string_lossy().into_owned(),
        );
        let (first, second) =
            tokio::join!(manager.rebuild(&old, &specs), manager.rebuild(&old, &specs));
        let (rebuilt, duplicate) = if first.is_ok() {
            (first, second)
        } else {
            (second, first)
        };
        let rebuilt = rebuilt.unwrap();
        assert!(matches!(duplicate, Err(TerminalError::Conflict(_))));
        assert_eq!(rebuilt.id, old.id);
        assert_eq!(rebuilt.name, old.name);
        assert_eq!(rebuilt.workspace_root, old.workspace_root);
        assert_eq!(rebuilt.created_at, old.created_at);
        assert!(rebuilt.revision > old.revision);
        let mut expected_layout = old.layout.clone();
        for (index, replacement) in rebuilt.panes.iter().enumerate() {
            assert_ne!(replacement.id, old.panes[index].id);
            assert_eq!(replacement.status, TerminalPaneStatus::Running);
            assert_eq!(replacement.exit_code, None);
            // Runtime responses intentionally redact environment variables.
            let mut expected_spec = specs[index].clone();
            expected_spec.environment.clear();
            assert_eq!(
                client.get(&replacement.id).await.unwrap().unwrap().spec(),
                expected_spec
            );
            replace_layout_leaf(
                &mut expected_layout,
                &old.panes[index].id,
                &TerminalLayout::Pane {
                    pane_id: replacement.id.clone(),
                },
            );
            assert!(client.get(&old.panes[index].id).await.unwrap().is_none());
        }
        assert_eq!(rebuilt.layout, expected_layout);
        tokio::time::timeout(Duration::from_secs(3), async {
            while std::fs::read_to_string(&output).unwrap_or_default() != "preserved" {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(manager.get_snapshot(&old.id).unwrap(), rebuilt);
        assert!(matches!(
            manager.rebuild(&old, &specs).await,
            Err(TerminalError::Conflict(_))
        ));
        assert_eq!(client.list().await.unwrap().len(), 2);
        let reloaded =
            TerminalManager::persistent(directory.path().join("state"), client.clone()).unwrap();
        assert_eq!(reloaded.get_snapshot(&old.id).unwrap().panes, rebuilt.panes);
        stop_daemon(shutdown, daemon).await;
    }

    #[tokio::test]
    async fn rebuild_failure_keeps_old_metadata_and_cleans_partial_runtimes() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("terminald/daemon.sock");
        let (shutdown, daemon) = start_daemon(socket.clone()).await;
        let client = TerminaldClient::new(socket);
        let manager = TerminalManager::in_memory(client.clone());
        let mut old = tab_with(
            TerminalLayout::Split {
                axis: TerminalSplitAxis::Column,
                ratio: 0.5,
                first: Box::new(TerminalLayout::Pane {
                    pane_id: "one".into(),
                }),
                second: Box::new(TerminalLayout::Pane {
                    pane_id: "two".into(),
                }),
            },
            vec![
                pane("one", TerminalPaneStatus::Exited),
                pane("two", TerminalPaneStatus::Interrupted),
            ],
        );
        old.panes[1].shell = "/missing/rebuild-shell".into();
        manager.lock_state().unwrap().tabs.push(old.clone());
        let specs: Vec<_> = old.panes.iter().map(runtime_spec).collect();
        assert!(manager.rebuild(&old, &specs).await.is_err());
        assert_eq!(manager.get_snapshot(&old.id).unwrap(), old);
        assert!(client.list().await.unwrap().is_empty());
        // A stale metadata status must never authorize replacing a live runtime.
        client.create("one", &specs[0]).await.unwrap();
        assert!(matches!(
            manager.rebuild(&old, &specs).await,
            Err(TerminalError::Conflict(_))
        ));
        assert_eq!(client.list().await.unwrap().len(), 1);
        stop_daemon(shutdown, daemon).await;
    }
}
