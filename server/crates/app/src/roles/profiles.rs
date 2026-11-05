use super::*;
impl RoleService {
    pub(crate) async fn resolve_agent_launch(
        &self,
        id: &str,
        worktree_path: &str,
    ) -> Result<AgentLaunch, ServiceError> {
        let requested = self
            .node
            .canonical_directory(Path::new(worktree_path))
            .await?;
        let projects = self.projects.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(ServiceError::Invalid(format!(
                "agent cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        self.resolve_agent_profile(id).await
    }
    pub(crate) async fn resolve_agent_profile(
        &self,
        id: &str,
    ) -> Result<AgentLaunch, ServiceError> {
        if let Some(role) = id.strip_prefix("role:") {
            return self.role_cli_launch(&self.agent_role(role)?).await;
        }
        self.agents.resolve_agent_profile(id).await
    }
    pub(crate) async fn resolve_terminal_rebuild_launch(
        &self,
        pane: &aow_api_contract::terminal::TerminalPane,
        workspace_root: &str,
    ) -> Result<AgentLaunch, ServiceError> {
        let profile_id = if let Some(id) = &pane.agent_profile_id {
            id.clone()
        } else {
            // Older metadata only saved the product type. Recover the profile
            // only when its executable and full launch arguments identify it.
            let candidates = self
                .node
                .agent_node()
                .await?
                .match_terminal_profiles(&aow_host_client::agents::MatchTerminalProfile {
                    profiles: self.agents.agents().await?,
                    agent: pane.agent_id.clone().unwrap_or_default(),
                    executable: pane.shell.clone(),
                    arguments: pane.arguments.clone(),
                })
                .await
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            if candidates.len() != 1 {
                return Err(ServiceError::Invalid(
                    aow_i18n::text(
                        "server.unable_to_identify_the_terminal_s_original_agent_configuration",
                    )
                    .into(),
                ));
            }
            candidates[0].clone()
        };
        let launch = self
            .resolve_agent_launch(&profile_id, workspace_root)
            .await?;
        if Some(launch.agent_type.id()) != pane.agent_id.as_deref() {
            return Err(ServiceError::Invalid(
                aow_i18n::text(
                    "server.the_original_agent_configuration_s_type_changed_cannot_rebuild",
                )
                .into(),
            ));
        }
        Ok(launch)
    }
}
