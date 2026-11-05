//! Typed HTTP management contract; ACP conversation messages use the ACP SDK.
use crate::{HostClient, HostError};
use aow_host_contract::acp::AgentInfo;
use serde::Deserialize;

pub use aow_host_contract::acp::{AcpInstance, AcpInstanceSpec, AcpInstanceStatus};
impl HostClient {
    pub async fn acp_agents(&self) -> Result<Vec<AgentInfo>, HostError> {
        #[derive(Deserialize)]
        struct Agents {
            agents: Vec<AgentInfo>,
        }
        Ok(self.get_json::<Agents>("/v1/acp/agents").await?.agents)
    }
    pub async fn acp_instances(&self) -> Result<Vec<AcpInstance>, HostError> {
        #[derive(Deserialize)]
        struct Instances {
            instances: Vec<AcpInstance>,
        }
        Ok(self
            .get_json::<Instances>("/v1/acp/instances")
            .await?
            .instances)
    }
    pub async fn create_acp_instance(
        &self,
        id: &str,
        spec: &AcpInstanceSpec,
    ) -> Result<AcpInstance, HostError> {
        self.put_json(&instance_path(id)?, spec).await
    }
    pub async fn acp_instance(&self, id: &str) -> Result<AcpInstance, HostError> {
        self.get_json(&instance_path(id)?).await
    }
    pub async fn delete_acp_instance(&self, id: &str) -> Result<(), HostError> {
        self.delete(&instance_path(id)?).await
    }
    pub async fn attach_acp(&self, id: &str) -> Result<crate::HostSocket, HostError> {
        self.websocket(&format!("{}/acp", instance_path(id)?)).await
    }
}
fn instance_path(id: &str) -> Result<String, HostError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(HostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid ACP instance ID",
        )));
    }
    Ok(format!("/v1/acp/instances/{id}"))
}
