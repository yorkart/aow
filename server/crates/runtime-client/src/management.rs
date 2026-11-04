//! Typed HTTP management contract; ACP conversation messages use the ACP SDK.
use crate::{RuntimeClient, RuntimeError};
use aow_acp::{AgentInfo, ExecutionEnvironment};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct AcpInstanceSpec {
    pub agent_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub execution_environment: Option<ExecutionEnvironment>,
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct AcpInstanceStatus {
    pub running: bool,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct AcpInstance {
    pub id: String,
    pub agent_id: String,
    pub cwd: PathBuf,
    pub pid: u32,
    pub attached: bool,
    pub status: AcpInstanceStatus,
    pub websocket_path: String,
    pub execution_home: Option<PathBuf>,
}
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
