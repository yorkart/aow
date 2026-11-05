use super::*;
impl AgentService {
    pub(crate) async fn resolve_agent_profile(
        &self,
        id: &str,
    ) -> Result<AgentLaunch, ServiceError> {
        let runtime = self.node.agent_node().await?;
        let agent = self
            .agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id && agent.available)
            .ok_or_else(|| ServiceError::AgentNotFound(id.into()))?;
        let request = aow_host_client::agents::ResolveAgent {
            profile: serde_json::from_value(serde_json::to_value(agent)?)?,
        };
        let mut launch = runtime
            .resolve_agent(&request)
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        launch.env.extend(self.callback_environment()?);
        Ok(launch)
    }
}
