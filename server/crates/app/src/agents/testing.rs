use super::*;
impl AgentService {
    pub(crate) async fn agents_in_path(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<AgentRegistration>, ServiceError> {
        let empty = tempfile::tempdir()?;
        let paths = if paths.is_empty() {
            vec![empty.path().into()]
        } else {
            paths.to_vec()
        };
        self.node.set_test_execution_path(paths.clone());
        self.node
            .agent_node()
            .await?
            .set_execution_path(&paths)
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        self.agents().await
    }
}
pub(crate) async fn resolve_on_node(
    agent: AgentRegistration,
    paths: &[PathBuf],
) -> Result<AgentLaunch, ServiceError> {
    let services = crate::test_support::TestServices::in_memory();
    services.agents.agents_in_path(paths).await?;
    services
        .node
        .agent_node()
        .await?
        .resolve_agent(&aow_runtime_client::agents::ResolveAgent {
            profile: serde_json::from_value(serde_json::to_value(agent)?)?,
        })
        .await
        .map_err(|e| ServiceError::Invalid(e.to_string()))
}
