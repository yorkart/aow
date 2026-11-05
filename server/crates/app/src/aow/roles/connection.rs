//! Connection tests execute against the candidate environment on the node.
use super::{AgentRole, RoleRuntime};
use crate::{AppState, aow::AowError};
use std::time::Duration;
pub(super) async fn test(state: &AppState, role: &AgentRole) -> Result<(), AowError> {
    test_with_timeout(state, role, Duration::from_secs(60)).await
}
pub(super) async fn test_with_timeout(
    state: &AppState,
    role: &AgentRole,
    deadline: Duration,
) -> Result<(), AowError> {
    {
        let runtime = state.aow.agent_node().await?;
        let mut probe = role.clone();
        probe.config.instruction.clear();
        let launch = match &probe.config.runtime {
            RoleRuntime::Cli { agent_id } => {
                Some(state.aow.role_runtime_cli_launch(agent_id).await?)
            }
            _ => None,
        };
        let agent = match &probe.config.runtime {
            RoleRuntime::Acp { agent_id } => Some(agent_id),
            _ => None,
        };
        runtime
            .probe_role_environment(&aow_runtime_contract::environment::ProbeEnvironment {
                spec: super::node::spec(role),
                launch,
                acp_agent_id: agent.cloned(),
                timeout_ms: deadline.as_millis().min(120_000) as u64,
            })
            .await
            .map_err(|error| AowError::Invalid(format!("Connection test failed: {error}")))?;
        Ok(())
    }
}
