use crate::agents::*;
impl AgentService {
    pub(crate) async fn agent_node(&self) -> Result<aow_host_client::HostClient, ServiceError> {
        if let Some(runtime) = self.node.node() {
            return Ok(runtime.clone());
        }
        #[cfg(test)]
        {
            self.node.test_acp_client()
                .map_err(|e| ServiceError::Invalid(e.to_string()))?
                .test_host()
                .await
                .map_err(|e| ServiceError::Invalid(e.to_string()))
        }
        #[cfg(not(test))]
        {
            Err(ServiceError::Invalid("Host is unavailable".into()))
        }
    }
}

#[cfg(test)]
impl AgentService {
    pub(crate) async fn agents_in_path(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<AgentRegistration>, ServiceError> {
        let empty = tempfile::tempdir()?;
        let paths = if paths.is_empty() {
            vec![empty.path().to_owned()]
        } else {
            paths.to_vec()
        };
        self.inner.settings.lock().unwrap().execution_path = Some(paths.clone());
        self.node.agent_node()
            .await?
            .set_execution_path(&paths)
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        self.agents().await
    }
}
#[cfg(test)]
pub(crate) async fn resolve_on_node(
    agent: AgentRegistration,
    paths: &[PathBuf],
) -> Result<AgentLaunch, ServiceError> {
    let manager = AgentService::in_memory();
    manager.agents_in_path(paths).await?;
    manager.node.agent_node()
        .await?
        .resolve_agent(&aow_host_client::agents::ResolveAgent {
            profile: serde_json::from_value(serde_json::to_value(agent)?)?,
        })
        .await
        .map_err(|e| ServiceError::Invalid(e.to_string()))
}
