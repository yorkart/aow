//! Typed HTTP management contract; ACP conversation messages use the ACP SDK.
use crate::{RuntimeClient, RuntimeError};
use aow_runtime_contract::acp::AgentInfo;
use serde::Deserialize;

pub use aow_runtime_contract::acp::{AcpInstance, AcpInstanceSpec, AcpInstanceStatus};
impl RuntimeClient {
    pub async fn acp_agents(&self) -> Result<Vec<AgentInfo>, RuntimeError> {
        #[derive(Deserialize)]
        struct Agents {
            agents: Vec<AgentInfo>,
        }
        Ok(self.get_json::<Agents>("/v1/acp/agents").await?.agents)
    }
    pub async fn acp_instances(&self) -> Result<Vec<AcpInstance>, RuntimeError> {
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
    ) -> Result<AcpInstance, RuntimeError> {
        self.put_json(&instance_path(id)?, spec).await
    }
    pub async fn acp_instance(&self, id: &str) -> Result<AcpInstance, RuntimeError> {
        self.get_json(&instance_path(id)?).await
    }
    pub async fn delete_acp_instance(&self, id: &str) -> Result<(), RuntimeError> {
        self.delete(&instance_path(id)?).await
    }
    pub async fn attach_acp(&self, id: &str) -> Result<crate::RuntimeSocket, RuntimeError> {
        self.websocket(&format!("{}/acp", instance_path(id)?)).await
    }
}
fn instance_path(id: &str) -> Result<String, RuntimeError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid ACP instance ID",
        )));
    }
    Ok(format!("/v1/acp/instances/{id}"))
}
