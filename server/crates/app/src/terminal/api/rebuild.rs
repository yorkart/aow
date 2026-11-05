use super::super::rebuild::ensure_rebuildable;
use super::*;

pub(super) async fn rebuild_terminal(
    State(state): State<AppState>,
    AxumPath(tab_id): AxumPath<String>,
) -> Result<Json<TerminalTab>, HttpError> {
    // Finish committing or rolling back even if the browser disconnects.
    aow_i18n::spawn(async move {
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
                        aow_i18n::text("server.terminal_has_no_agent_configuration_cannot_rebuild")
                            .into(),
                    ))
                })?;
                if pane.agent_terminal.is_some()
                    && !state
                        .node
                        .agent_node()
                        .await
                        .map_err(crate::error::service_http_error)?
                        .agent_capabilities()
                        .await
                        .map_err(|e| HttpError::internal(e.to_string()))?
                        .iter()
                        .any(|cap| cap.id == agent_id && cap.interactive)
                {
                    return Err(terminal_http_error(TerminalError::Invalid(
                        "Agent does not support interactive initialization".into(),
                    )));
                }
                // Credentials and PATH come from the current workspace profile;
                // keep the saved command/arguments (including explicit resume).
                spec.environment = state
                    .roles
                    .resolve_terminal_rebuild_launch(pane, &previous.workspace_root)
                    .await
                    .map_err(crate::error::service_http_error)?
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
                let agent = pane.agent_id.clone().expect("validated agent identity");
                let manager = state.terminals.clone();
                let pane_id = pane.id.clone();
                aow_i18n::spawn(async move {
                    if let Err(error) = manager
                        .initialize_agent_lifecycle(
                            &pane_id,
                            &agent,
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
