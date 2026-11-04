//! Test role environments are fixture data; probes still execute in the external Runtime.
use super::{AgentRole, RoleRuntime};
use crate::{AppState, aow::AowError};
use std::time::Duration;
pub(super) async fn test_with_timeout(
    state: &AppState,
    role: &AgentRole,
    deadline: Duration,
) -> Result<(), AowError> {
    let result = tokio::time::timeout(deadline, async {
        match &role.config.runtime {
            RoleRuntime::Cli { .. } => {
                let mut probe = role.clone();
                probe.config.instruction.clear();
                let launch = state.aow.role_cli_launch(&probe).await?;
                state
                    .aow
                    .agent_node()
                    .await?
                    .probe_agent(&aow_runtime_client::agents::ProbeAgent {
                        launch,
                        timeout_ms: deadline.as_millis().min(120_000) as u64,
                    })
                    .await
                    .map_err(|e| AowError::Invalid(e.to_string()))
            }
            RoleRuntime::Acp { agent_id } => state
                .aow
                .role_acp_service()
                .map_err(|e| AowError::Invalid(e.to_string()))?
                .probe(
                    agent_id,
                    state.aow.role_home(&role.id)?,
                    state.aow.role_acp_environment(role)?,
                )
                .await
                .map_err(|e| AowError::Invalid(e.to_string())),
        }
    })
    .await
    .map_err(|_| AowError::Invalid("Connection test timed out".into()))?;
    result.map_err(|e| AowError::Invalid(format!("Connection test failed: {e}")))
}
