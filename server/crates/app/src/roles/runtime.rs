//! Resolve role references through the existing runtime configuration owners.
use super::{RoleRuntime, model::HomeLayout};
use crate::agents::AgentLaunch;
use crate::error::ServiceError;
use crate::roles::RoleService;
use anyhow::Result;
use aow_host_client::acp::AcpClient;

impl RoleService {
    pub(super) async fn role_layout(&self, id: &str) -> Result<HomeLayout, ServiceError> {
        let layout = self
            .node
            .agent_node()
            .await?
            .agent_role_layout(&aow_host_client::agents::ResolveRoleLayout {
                agent_id: id.into(),
            })
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        serde_json::from_value(serde_json::Value::String(layout)).map_err(Into::into)
    }
    pub(super) async fn role_runtime_cli_launch(
        &self,
        id: &str,
    ) -> Result<AgentLaunch, ServiceError> {
        crate::validation::validate_id(id)?;
        let agent = self
            .agents
            .agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id && agent.available)
            .ok_or_else(|| {
                ServiceError::Invalid(
                    "Select an available Agent CLI instance in Settings → Runtime".into(),
                )
            })?;
        self.node
            .agent_node()
            .await?
            .resolve_agent(&aow_host_client::agents::ResolveAgent {
                profile: serde_json::from_value(serde_json::to_value(agent)?)?,
            })
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))
    }

    pub(super) fn role_acp_service(&self) -> Result<AcpClient> {
        if let Some(runtime) = self.node.node() {
            return Ok(AcpClient::new(runtime.clone()));
        }
        #[cfg(test)]
        {
            self.node.test_acp_client()
        }
        #[cfg(not(test))]
        {
            anyhow::bail!("Host is unavailable")
        }
    }

    pub(super) async fn role_runtime_layout(
        &self,
        runtime: &RoleRuntime,
    ) -> Result<HomeLayout, ServiceError> {
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
                let service = self
                    .role_acp_service()
                    .map_err(ServiceError::Configuration)?;
                if !service
                    .agents()
                    .await
                    .map_err(ServiceError::Configuration)?
                    .iter()
                    .any(|agent| {
                        agent.id == *agent_id
                            && agent.configured
                            && agent.installed
                            && agent.supported
                    })
                {
                    return Err(ServiceError::Invalid(
                        "Select an installed ACP instance in Settings → Runtime".into(),
                    ));
                }
                self.role_layout(agent_id).await?
            }
        };
        Ok(layout)
    }
}
