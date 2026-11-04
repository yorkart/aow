//! Resolve role references through the existing runtime configuration owners.
use super::{RoleRuntime, model::HomeLayout};
use crate::aow::{AgentLaunch, AowError, AowManager};
use anyhow::Result;
use aow_runtime_client::acp::AcpClient;

impl AowManager {
    pub(super) async fn role_layout(&self, id: &str) -> Result<HomeLayout, AowError> {
        let layout = self
            .agent_node()
            .await?
            .agent_role_layout(&aow_runtime_client::agents::ResolveRoleLayout {
                agent_id: id.into(),
            })
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))?;
        serde_json::from_value(serde_json::Value::String(layout)).map_err(Into::into)
    }
    pub(super) async fn role_runtime_cli_launch(&self, id: &str) -> Result<AgentLaunch, AowError> {
        crate::aow::validation::validate_id(id)?;
        let agent = self
            .agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id && agent.source == "configured" && agent.available)
            .ok_or_else(|| {
                AowError::Invalid(
                    "Select an available runtime configured in Settings → Agents".into(),
                )
            })?;
        self.agent_node()
            .await?
            .resolve_agent(&aow_runtime_client::agents::ResolveAgent {
                profile: serde_json::from_value(serde_json::to_value(agent)?)?,
            })
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))
    }

    pub(super) fn role_acp_service(&self) -> Result<AcpClient> {
        if let Some(runtime) = &self.inner.runtime_client {
            return Ok(AcpClient::new(runtime.clone()));
        }
        #[cfg(test)]
        {
            self.test_acp_client()
        }
        #[cfg(not(test))]
        {
            anyhow::bail!("Runtime is unavailable")
        }
    }

    pub(super) async fn role_runtime_layout(
        &self,
        runtime: &RoleRuntime,
    ) -> Result<HomeLayout, AowError> {
        let layout = match runtime {
            RoleRuntime::Cli { agent_id } => {
                self.role_layout(
                    self.role_runtime_cli_launch(agent_id)
                        .await?
                        .agent_type
                        .id(),
                )
                .await?
            }
            RoleRuntime::Acp { agent_id } => {
                let service = self.role_acp_service().map_err(AowError::Configuration)?;
                if !service
                    .agents()
                    .await
                    .map_err(AowError::Configuration)?
                    .iter()
                    .any(|agent| {
                        agent.id == *agent_id
                            && agent.configured
                            && agent.installed
                            && agent.supported
                    })
                {
                    return Err(AowError::Invalid(
                        "Select an installed runtime configured in Settings → ACP".into(),
                    ));
                }
                self.role_layout(agent_id).await?
            }
        };
        Ok(layout)
    }
}
