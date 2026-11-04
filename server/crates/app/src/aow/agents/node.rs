use super::super::*;
impl AowManager {
    pub(crate) async fn agent_node(&self) -> Result<aow_runtime_client::RuntimeClient, AowError> {
        if let Some(runtime) = &self.inner.runtime_client {
            return Ok(runtime.clone());
        }
        #[cfg(test)]
        {
            self.test_acp_client()
                .map_err(|e| AowError::Invalid(e.to_string()))?
                .test_runtime()
                .await
                .map_err(|e| AowError::Invalid(e.to_string()))
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Runtime is unavailable".into()))
        }
    }
}

#[cfg(test)]
impl AowManager {
    pub(crate) async fn agents_in_path(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<AgentRegistration>, AowError> {
        let empty = tempfile::tempdir()?;
        let paths = if paths.is_empty() {
            vec![empty.path().to_owned()]
        } else {
            paths.to_vec()
        };
        self.inner.settings.lock().unwrap().execution_path = Some(paths.clone());
        self.agent_node()
            .await?
            .set_execution_path(&paths)
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))?;
        self.agents().await
    }
}
#[cfg(test)]
pub(crate) async fn resolve_on_node(
    agent: AgentRegistration,
    paths: &[PathBuf],
) -> Result<AgentLaunch, AowError> {
    let manager = AowManager::in_memory();
    manager.agents_in_path(paths).await?;
    manager
        .agent_node()
        .await?
        .resolve_agent(&aow_runtime_client::agents::ResolveAgent {
            profile: serde_json::from_value(serde_json::to_value(agent)?)?,
        })
        .await
        .map_err(|e| AowError::Invalid(e.to_string()))
}
