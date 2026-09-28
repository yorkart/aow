use super::super::rebuild::ensure_rebuildable;
use super::*;
use aow_agents::Agent;

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
